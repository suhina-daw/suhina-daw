// Copyright (C) 2026 The Suhina contributors
// SPDX-License-Identifier: GPL-3.0-or-later

use super::*;

fn wait<F: std::future::Future>(future: F) -> F::Output {
    struct Wake(std::thread::Thread);
    impl std::task::Wake for Wake {
        fn wake(self: std::sync::Arc<Self>) {
            self.0.unpark();
        }
    }
    let waker = std::task::Waker::from(std::sync::Arc::new(Wake(std::thread::current())));
    let mut cx = std::task::Context::from_waker(&waker);
    let mut future = std::pin::pin!(future);
    loop {
        if let std::task::Poll::Ready(value) = future.as_mut().poll(&mut cx) {
            return value;
        }
        std::thread::park_timeout(std::time::Duration::from_millis(100));
    }
}

/// How many of a widget's pixels are not transparent, and their largest alpha.
fn cover(strands: &Strands, id: egui::Id) -> (usize, u8) {
    let gpu = &strands.gpu;
    let guard = strands.state.lock().unwrap();
    let texture = &guard.as_ref().unwrap().targets[&id].texture;
    let row = (texture.width() * 4).div_ceil(256) * 256;
    let buffer = gpu.device.create_buffer(&wgpu::BufferDescriptor {
        label: Some("test-only strands readback"),
        size: u64::from(row) * u64::from(texture.height()),
        usage: wgpu::BufferUsages::COPY_DST | wgpu::BufferUsages::MAP_READ,
        mapped_at_creation: false,
    });
    let mut encoder = gpu.device.create_command_encoder(&Default::default());
    encoder.copy_texture_to_buffer(
        texture.as_image_copy(),
        wgpu::TexelCopyBufferInfo {
            buffer: &buffer,
            layout: wgpu::TexelCopyBufferLayout { offset: 0, bytes_per_row: Some(row), rows_per_image: None },
        },
        texture.size(),
    );
    gpu.queue.submit([encoder.finish()]);
    let (send, receive) = std::sync::mpsc::channel();
    buffer.slice(..).map_async(wgpu::MapMode::Read, move |result| {
        let _ = send.send(result);
    });
    gpu.device.poll(wgpu::PollType::wait_indefinitely()).unwrap();
    receive.recv().unwrap().unwrap();
    let mapped = buffer.slice(..).get_mapped_range().unwrap();
    let mut count = 0;
    let mut largest = 0;
    for line in mapped.chunks(row as usize) {
        for pixel in line[..texture.width() as usize * 4].chunks(4) {
            if pixel[3] > 0 {
                count += 1;
                largest = largest.max(pixel[3]);
            }
            assert!(pixel[0] <= pixel[3] && pixel[1] <= pixel[3] && pixel[2] <= pixel[3], "premultiplied");
        }
    }
    (count, largest)
}

#[test]
#[ignore = "requires a native GPU; run explicitly for rendering changes"]
fn the_dots_draw_into_the_widgets_own_texture_and_sound_adds_to_them() {
    let instance = wgpu::Instance::default();
    let gpu = wait(egui_wgpu::RenderState::create(&Default::default(), &instance, None, Default::default())).unwrap();
    let strands = Strands::new(gpu);
    let ctx = egui::Context::default();
    let id = egui::Id::unique("strands test");
    let colors = [[1.0, 0.98, 0.93, 1.0], [0.87, 0.74, 0.53, 1.0], [0.5, 0.34, 0.21, 1.0], [0.78, 0.5, 0.2, 1.0]];
    let rest = Frame { colors, ..Frame::default() };
    let image = strands.draw(&ctx, id, [880, 236], &rest).unwrap();
    let (at_rest, _) = cover(&strands, id);
    assert!(at_rest > 5_000, "the ribbons show at rest: {at_rest} pixels");
    let sounding = Frame { modes: [0.9, 0.7, 0.5, 0.3, 0.0, 0.0], since: 0.3, velocity: 1.0, ..rest };
    assert_eq!(strands.draw(&ctx, id, [880, 236], &sounding), Some(image), "the same widget keeps its texture");
    let (playing, largest) = cover(&strands, id);
    assert!(playing > at_rest, "sound raises more ribbons: {playing} against {at_rest}");
    assert!(largest > 40, "the dots add up where they overlap: {largest}");
    // A new size keeps the image's id.
    assert_eq!(strands.draw(&ctx, id, [440, 118], &rest), Some(image));
    assert_eq!(strands.state.lock().unwrap().as_ref().unwrap().targets[&id].size, [440, 118]);
    let nothing = Frame { presence: 0.0, ..rest };
    strands.draw(&ctx, id, [440, 118], &nothing).unwrap();
    assert_eq!(cover(&strands, id).0, 0, "no presence, no dots");
}
