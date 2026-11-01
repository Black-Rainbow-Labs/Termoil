//! Copying a GPU texture back to memory, for the pixel tests and screenshots.

use crate::{Image, RenderError};

/// Reads an RGBA8 texture into an image.
pub fn read_texture(device: &wgpu::Device, queue: &wgpu::Queue, texture: &wgpu::Texture) -> Result<Image, RenderError> {
    let (width, height) = (texture.width(), texture.height());
    let bytes_per_row = (width * 4).next_multiple_of(256);
    let buffer = device.create_buffer(&wgpu::BufferDescriptor {
        label: Some("terminus readback"),
        size: u64::from(bytes_per_row) * u64::from(height),
        usage: wgpu::BufferUsages::COPY_DST | wgpu::BufferUsages::MAP_READ,
        mapped_at_creation: false,
    });
    let mut encoder =
        device.create_command_encoder(&wgpu::CommandEncoderDescriptor { label: Some("terminus readback") });
    encoder.copy_texture_to_buffer(
        wgpu::TexelCopyTextureInfo {
            texture,
            mip_level: 0,
            origin: wgpu::Origin3d::ZERO,
            aspect: wgpu::TextureAspect::All,
        },
        wgpu::TexelCopyBufferInfo {
            buffer: &buffer,
            layout: wgpu::TexelCopyBufferLayout { offset: 0, bytes_per_row: Some(bytes_per_row), rows_per_image: None },
        },
        wgpu::Extent3d { width, height, depth_or_array_layers: 1 },
    );
    queue.submit([encoder.finish()]);
    buffer.slice(..).map_async(wgpu::MapMode::Read, |_| {});
    device.poll(wgpu::PollType::wait_indefinitely()).map_err(|e| RenderError::Gpu(format!("{e:?}")))?;
    let mapped = buffer.slice(..).get_mapped_range().map_err(|e| RenderError::Gpu(format!("{e:?}")))?;
    let mut rgba = Vec::with_capacity((width * height * 4) as usize);
    for row in mapped.chunks_exact(bytes_per_row as usize) {
        rgba.extend_from_slice(&row[..(width * 4) as usize]);
    }
    drop(mapped);
    buffer.unmap();
    Ok(Image { width, height, rgba })
}
