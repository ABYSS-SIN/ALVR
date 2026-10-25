//! Renders the settings tab off-screen to PNG files, to check the layout without opening the
//! dashboard: `ALVR_SNAPSHOT_DIR=some/dir cargo test -p alvr_dashboard settings_snapshot`.

use super::SettingsTab;
use alvr_session::SessionConfig;
use eframe::egui::{self, Color32, Event, Frame, Rect, vec2};
use egui_wgpu::{RenderState, RendererOptions, ScreenDescriptor, WgpuConfiguration, wgpu};

const SIZE: [u32; 2] = [1600, 1000];
const PIXELS_PER_POINT: f32 = 1.0;

struct Offscreen {
    ctx: egui::Context,
    state: RenderState,
    texture: wgpu::Texture,
    readback: wgpu::Buffer,
}

impl Offscreen {
    fn new() -> Self {
        let config = WgpuConfiguration::default();
        let state = pollster::block_on(async {
            let instance = config.wgpu_setup.new_instance().await;
            RenderState::create(&config, &instance, None, RendererOptions::default()).await
        })
        .unwrap();

        let texture = state.device.create_texture(&wgpu::TextureDescriptor {
            label: None,
            size: wgpu::Extent3d {
                width: SIZE[0],
                height: SIZE[1],
                depth_or_array_layers: 1,
            },
            mip_level_count: 1,
            sample_count: 1,
            dimension: wgpu::TextureDimension::D2,
            format: state.target_format,
            usage: wgpu::TextureUsages::RENDER_ATTACHMENT | wgpu::TextureUsages::COPY_SRC,
            view_formats: &[],
        });
        let readback = state.device.create_buffer(&wgpu::BufferDescriptor {
            label: None,
            size: (SIZE[0] * SIZE[1] * 4) as u64,
            usage: wgpu::BufferUsages::COPY_DST | wgpu::BufferUsages::MAP_READ,
            mapped_at_creation: false,
        });

        let ctx = egui::Context::default();
        ctx.set_pixels_per_point(PIXELS_PER_POINT);
        alvr_gui_common::theme::set_theme(&ctx);

        Self {
            ctx,
            state,
            texture,
            readback,
        }
    }

    // Runs the ui twice (see the overlay renderer: new glyphs reach the atlas one output late)
    fn frame(&mut self, events: Vec<Event>, mut ui: impl FnMut(&mut egui::Ui)) -> Vec<u8> {
        let mut run = |events: Vec<Event>| {
            let raw_input = egui::RawInput {
                screen_rect: Some(Rect::from_min_size(
                    egui::Pos2::ZERO,
                    vec2(SIZE[0] as f32, SIZE[1] as f32) / PIXELS_PER_POINT,
                )),
                events,
                ..Default::default()
            };
            self.ctx.run_ui(raw_input, |root| {
                Frame::new()
                    .fill(Color32::from_rgb(30, 30, 30))
                    .inner_margin(12.0)
                    .show(root, |content| {
                        content.set_min_size(content.available_size());
                        ui(content);
                    });
            })
        };

        let warmup = run(events);
        self.ctx
            .tessellate(warmup.shapes.clone(), warmup.pixels_per_point);
        let output = run(vec![]);

        let jobs = self.ctx.tessellate(output.shapes, output.pixels_per_point);
        let screen = ScreenDescriptor {
            size_in_pixels: SIZE,
            pixels_per_point: PIXELS_PER_POINT,
        };

        let device = &self.state.device;
        let queue = &self.state.queue;
        let mut renderer = self.state.renderer.write();

        for delta in [&warmup.textures_delta, &output.textures_delta] {
            for (id, delta) in &delta.set {
                renderer.update_texture(device, queue, *id, delta);
            }
        }

        let mut encoder = device.create_command_encoder(&Default::default());
        let extra = renderer.update_buffers(device, queue, &mut encoder, &jobs, &screen);
        let view = self.texture.create_view(&Default::default());
        {
            let mut pass = encoder
                .begin_render_pass(&wgpu::RenderPassDescriptor {
                    label: None,
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
                    bytes_per_row: Some(SIZE[0] * 4),
                    rows_per_image: Some(SIZE[1]),
                },
            },
            wgpu::Extent3d {
                width: SIZE[0],
                height: SIZE[1],
                depth_or_array_layers: 1,
            },
        );
        queue.submit(extra.into_iter().chain([encoder.finish()]));
        drop(renderer);

        let slice = self.readback.slice(..);
        slice.map_async(wgpu::MapMode::Read, |result| result.unwrap());
        device
            .poll(wgpu::PollType::wait_indefinitely())
            .unwrap();
        let pixels = slice.get_mapped_range().to_vec();
        self.readback.unmap();

        pixels
    }
}

fn save(path: &std::path::Path, pixels: &[u8]) {
    let file = std::fs::File::create(path).unwrap();
    let mut encoder = png::Encoder::new(file, SIZE[0], SIZE[1]);
    encoder.set_color(png::ColorType::Rgba);
    encoder.set_depth(png::BitDepth::Eight);
    encoder.write_header().unwrap().write_image_data(pixels).unwrap();
}

#[test]
fn settings_snapshot() {
    let Ok(dir) = std::env::var("ALVR_SNAPSHOT_DIR") else {
        return;
    };
    let dir = std::path::PathBuf::from(dir);
    let tabs = std::env::var("ALVR_SNAPSHOT_TABS").unwrap_or_else(|_| "presets,video".into());

    let mut screen = Offscreen::new();
    let mut tab = SettingsTab::new();
    tab.update_session(&SessionConfig::default().session_settings);

    // "search:text" renders the search results instead of a tab
    for id in tabs.split(',') {
        if let Some(query) = id.strip_prefix("search:") {
            tab.set_search(query);
        } else {
            tab.set_search("");
            tab.select_tab(id);
        }
        let id = id.replace(':', "_");
        let id = id.as_str();
        // The first frames settle the layout
        let mut pixels = vec![];
        for _ in 0..3 {
            pixels = screen.frame(vec![], |ui| _ = tab.ui(ui));
        }
        save(&dir.join(format!("settings_{id}.png")), &pixels);
    }
}
