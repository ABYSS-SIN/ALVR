//! Runs egui off-screen and returns the frame as RGBA8 pixels (what SetOverlayRaw wants).

use anyhow::{Result, anyhow};
use egui::{Color32, Context, Event, Rect, vec2};
use egui_wgpu::{
    RenderState, RendererOptions, ScreenDescriptor, WgpuConfiguration,
    wgpu,
};
use std::time::Instant;

pub struct Gui {
    pub ctx: Context,
    state: RenderState,
    texture: wgpu::Texture,
    readback: wgpu::Buffer,
    size: [u32; 2],
    pixels_per_point: f32,
    start: Instant,
    pixels: Vec<u8>,
}

impl Gui {
    /// `size` is in pixels, its width must be a multiple of 64 (readback row alignment).
    pub fn new(size: [u32; 2], pixels_per_point: f32) -> Result<Self> {
        assert_eq!(size[0] % 64, 0);

        let config = WgpuConfiguration::default();
        let state = pollster::block_on(async {
            let instance = config.wgpu_setup.new_instance().await;
            RenderState::create(&config, &instance, None, RendererOptions::default()).await
        })?;

        let texture = state.device.create_texture(&wgpu::TextureDescriptor {
            label: Some("overlay"),
            size: wgpu::Extent3d {
                width: size[0],
                height: size[1],
                depth_or_array_layers: 1,
            },
            mip_level_count: 1,
            sample_count: 1,
            dimension: wgpu::TextureDimension::D2,
            format: state.target_format,
            usage: wgpu::TextureUsages::RENDER_ATTACHMENT | wgpu::TextureUsages::COPY_SRC,
            view_formats: &[],
        });
        if state.target_format != wgpu::TextureFormat::Rgba8Unorm {
            return Err(anyhow!("unexpected target format {:?}", state.target_format));
        }

        let readback = state.device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("overlay readback"),
            size: (size[0] * size[1] * 4) as u64,
            usage: wgpu::BufferUsages::COPY_DST | wgpu::BufferUsages::MAP_READ,
            mapped_at_creation: false,
        });

        let ctx = Context::default();
        ctx.set_pixels_per_point(pixels_per_point);
        alvr_gui_common::theme::set_theme(&ctx);
        // Big targets: the panel is operated with a VR laser pointer
        ctx.global_style_mut(|style| {
            style.spacing.interact_size = vec2(60.0, 28.0);
            style.spacing.button_padding = vec2(14.0, 10.0);
            style.spacing.item_spacing = vec2(14.0, 10.0);
        });

        Ok(Self {
            ctx,
            state,
            texture,
            readback,
            size,
            pixels_per_point,
            start: Instant::now(),
            pixels: vec![0; (size[0] * size[1] * 4) as usize],
        })
    }

    pub fn size(&self) -> [u32; 2] {
        self.size
    }

    /// Runs one egui frame and renders it. `events` use points coordinates.
    pub fn frame(&mut self, events: Vec<Event>, mut ui: impl FnMut(&mut egui::Ui)) -> &[u8] {
        let size_points = vec2(self.size[0] as f32, self.size[1] as f32) / self.pixels_per_point;
        let mut run = |events: Vec<Event>, time: f64| {
            let raw_input = egui::RawInput {
                screen_rect: Some(Rect::from_min_size(egui::Pos2::ZERO, size_points)),
                time: Some(time),
                events,
                ..Default::default()
            };
            self.ctx.run_ui(raw_input, |ui_root| {
                egui::Frame::new()
                    .fill(Color32::from_rgb(24, 24, 28))
                    .inner_margin(24.0)
                    .show(ui_root, |content| {
                        content.set_min_size(content.available_size());
                        ui(content);
                    });
            })
        };

        // Glyphs first used by this frame (new digits in the stats...) only reach the font atlas
        // after tessellation, so the texture update is in the *next* output: drawing the first one
        // shows them blank for a frame. Run twice, upload the first output's textures, draw the second.
        let time = self.start.elapsed().as_secs_f64();
        let warmup = run(events, time);
        self.ctx.tessellate(warmup.shapes, warmup.pixels_per_point);
        let output = run(vec![], time);
        let mut textures_delta = warmup.textures_delta;
        textures_delta.append(output.textures_delta.clone());

        let jobs = self.ctx.tessellate(output.shapes, output.pixels_per_point);
        let screen = ScreenDescriptor {
            size_in_pixels: self.size,
            pixels_per_point: self.pixels_per_point,
        };

        let device = &self.state.device;
        let queue = &self.state.queue;
        let mut renderer = self.state.renderer.write();

        for (id, delta) in &textures_delta.set {
            renderer.update_texture(device, queue, *id, delta);
        }

        let mut encoder = device.create_command_encoder(&Default::default());
        let extra = renderer.update_buffers(device, queue, &mut encoder, &jobs, &screen);

        let view = self.texture.create_view(&Default::default());
        {
            let mut pass = encoder
                .begin_render_pass(&wgpu::RenderPassDescriptor {
                    label: Some("overlay"),
                    color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                        view: &view,
                        depth_slice: None,
                        resolve_target: None,
                        ops: wgpu::Operations {
                            load: wgpu::LoadOp::Clear(wgpu::Color::BLACK),
                            store: wgpu::StoreOp::Store,
                        },
                    })],
                    ..Default::default()
                })
                .forget_lifetime();
            renderer.render(&mut pass, &jobs, &screen);
        }

        encoder.copy_texture_to_buffer(
            self.texture.as_image_copy(),
            wgpu::TexelCopyBufferInfo {
                buffer: &self.readback,
                layout: wgpu::TexelCopyBufferLayout {
                    offset: 0,
                    bytes_per_row: Some(self.size[0] * 4),
                    rows_per_image: Some(self.size[1]),
                },
            },
            wgpu::Extent3d {
                width: self.size[0],
                height: self.size[1],
                depth_or_array_layers: 1,
            },
        );

        queue.submit(extra.into_iter().chain([encoder.finish()]));

        for id in &textures_delta.free {
            renderer.free_texture(id);
        }
        drop(renderer);

        let slice = self.readback.slice(..);
        slice.map_async(wgpu::MapMode::Read, |result| result.unwrap());
        device
            .poll(wgpu::PollType::wait_indefinitely())
            .expect("GPU poll failed");
        self.pixels.copy_from_slice(&slice.get_mapped_range());
        self.readback.unmap();

        &self.pixels
    }
}
