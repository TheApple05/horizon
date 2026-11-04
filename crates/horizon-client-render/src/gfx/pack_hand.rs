//! The first-person hand and held items drawn by a shader pack's `gbuffers_hand` (spec 2.3):
//! `Frame::hand`'s entity vertices, read as entities are (`pack_entities::inputs`), with the
//! hand's own projection as `gl_ProjectionMatrix` (5.2: `gbufferProjection` stays the world's).

use glam::Mat4;

use super::Vertex;

/// `MC_HAND_DEPTH`'s value (1.5): the hand's depth is scaled by it through its projection, so
/// that it sits in front of the world in the shared depth buffer.
const HAND_DEPTH: f32 = 0.125;

/// `HzHand`: the hand's projection and its inverse.
const UNIFORM: u64 = 2 * 64;

/// What `gbuffers_hand` reads: the entity inputs, and the hand's projection from set 1.
pub fn inputs() -> horizon_shaders::GbuffersInputs {
    let mut inputs = super::pack_entities::inputs();
    inputs.declarations.push_str(
        "layout(set = 1, binding = 1) uniform HzHand { mat4 hz_hand_projection; mat4 hz_hand_projection_inverse; };\n",
    );
    inputs.body.push_str(
        "    hz_ProjectionMatrix = hz_hand_projection;\n    \
         hz_ProjectionMatrixInverse = hz_hand_projection_inverse;\n    \
         hz_ModelViewProjectionMatrix = hz_hand_projection * hz_ModelViewMatrix;\n",
    );
    inputs
}

/// The hand's projection in GL's conventions: `hud_fov` degrees, depth scaled by
/// `MC_HAND_DEPTH` (the view bob is in `gbufferModelView`).
pub fn projection(hud_fov: f32, aspect: f32, far: f32) -> Mat4 {
    Mat4::from_scale(glam::Vec3::new(1.0, 1.0, HAND_DEPTH))
        * Mat4::perspective_rh_gl(hud_fov.to_radians(), aspect, 0.05, far)
}

/// The hand's vertices (also read at set 1 for normals and tangents), its uniform and quad
/// indices.
pub struct PackHand {
    pub layout: wgpu::BindGroupLayout,
    vertices: wgpu::Buffer,
    uniform: wgpu::Buffer,
    pub group: wgpu::BindGroup,
    pub indices: wgpu::Buffer,
    capacity: u32,
    /// Quads uploaded this frame.
    pub quads: u32,
    draw_layout: wgpu::BindGroupLayout,
    slots: wgpu::Buffer,
    pub draw_group: wgpu::BindGroup,
    stride: u32,
    /// Vertex ranges and their dynamic draw-uniform offsets.
    pub draws: Vec<(std::ops::Range<u32>, u32)>,
}

impl PackHand {
    pub fn new(device: &wgpu::Device, draw_layout: &wgpu::BindGroupLayout) -> Self {
        let entry = |binding, ty| wgpu::BindGroupLayoutEntry {
            binding,
            visibility: wgpu::ShaderStages::VERTEX,
            ty: wgpu::BindingType::Buffer {
                ty,
                has_dynamic_offset: false,
                min_binding_size: None,
            },
            count: None,
        };
        let layout = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
            label: Some("Shader pack hand"),
            entries: &[
                entry(0, wgpu::BufferBindingType::Storage { read_only: true }),
                entry(1, wgpu::BufferBindingType::Uniform),
            ],
        });
        let uniform = device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("Shader pack hand"),
            size: UNIFORM,
            usage: wgpu::BufferUsages::UNIFORM | wgpu::BufferUsages::COPY_DST,
            mapped_at_creation: false,
        });
        let capacity = 256;
        let (vertices, group) = Self::vertex_buffer(device, &layout, &uniform, capacity);
        let stride = device.limits().min_uniform_buffer_offset_alignment.max(32);
        let (slots, draw_group) = Self::slot_buffer(device, draw_layout, stride * 4);
        Self {
            indices: super::pack_entities::quad_indices(device, capacity),
            layout,
            vertices,
            uniform,
            group,
            capacity,
            quads: 0,
            draw_layout: draw_layout.clone(),
            slots,
            draw_group,
            stride,
            draws: Vec::new(),
        }
    }

    fn slot_buffer(
        device: &wgpu::Device,
        layout: &wgpu::BindGroupLayout,
        size: u32,
    ) -> (wgpu::Buffer, wgpu::BindGroup) {
        let slots = device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("Shader pack hand draw uniforms"),
            size: u64::from(size),
            usage: wgpu::BufferUsages::UNIFORM | wgpu::BufferUsages::COPY_DST,
            mapped_at_creation: false,
        });
        let group = device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: Some("Shader pack hand draw uniforms"),
            layout,
            entries: &[wgpu::BindGroupEntry {
                binding: 0,
                resource: wgpu::BindingResource::Buffer(wgpu::BufferBinding {
                    buffer: &slots,
                    offset: 0,
                    size: wgpu::BufferSize::new(32),
                }),
            }],
        });
        (slots, group)
    }

    fn vertex_buffer(
        device: &wgpu::Device,
        layout: &wgpu::BindGroupLayout,
        uniform: &wgpu::Buffer,
        quads: u32,
    ) -> (wgpu::Buffer, wgpu::BindGroup) {
        let buffer = device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("Shader pack hand vertices"),
            size: u64::from(quads) * 4 * std::mem::size_of::<Vertex>() as u64,
            usage: wgpu::BufferUsages::VERTEX
                | wgpu::BufferUsages::STORAGE
                | wgpu::BufferUsages::COPY_DST,
            mapped_at_creation: false,
        });
        let group = device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: Some("Shader pack hand"),
            layout,
            entries: &[
                wgpu::BindGroupEntry {
                    binding: 0,
                    resource: buffer.as_entire_binding(),
                },
                wgpu::BindGroupEntry {
                    binding: 1,
                    resource: uniform.as_entire_binding(),
                },
            ],
        });
        (buffer, group)
    }

    /// Uploads this frame's hand and its projection.
    pub fn prepare(
        &mut self,
        device: &wgpu::Device,
        queue: &wgpu::Queue,
        hand: &[Vertex],
        spans: &[super::pack_draw::EntitySpan],
        projection: Mat4,
    ) {
        self.draws.clear();
        self.quads = (hand.len() / 4) as u32;
        if self.quads == 0 {
            return;
        }
        if self.quads > self.capacity {
            self.capacity = self.quads.next_power_of_two();
            (self.vertices, self.group) =
                Self::vertex_buffer(device, &self.layout, &self.uniform, self.capacity);
            self.indices = super::pack_entities::quad_indices(device, self.capacity);
        }
        queue.write_buffer(&self.vertices, 0, bytemuck::cast_slice(hand));
        let matrices = [projection, projection.inverse()].map(|m| m.to_cols_array());
        queue.write_buffer(&self.uniform, 0, bytemuck::cast_slice(&matrices));
        let ranges = item_ranges(hand.len(), spans);
        let size = ranges.len() as u32 * self.stride;
        if u64::from(size) > self.slots.size() {
            (self.slots, self.draw_group) =
                Self::slot_buffer(device, &self.draw_layout, size.next_power_of_two());
        }
        let mut contents = vec![0; size as usize];
        for (index, (range, item_id)) in ranges.into_iter().enumerate() {
            let offset = index as u32 * self.stride;
            let bytes = super::pack_draw::encode(
                horizon_shaders::render_stage("HAND_SOLID"),
                0,
                -1,
                item_id,
                [0.0; 4],
            );
            contents[offset as usize..offset as usize + bytes.len()].copy_from_slice(&bytes);
            self.draws.push((range, offset));
        }
        queue.write_buffer(&self.slots, 0, &contents);
    }

    pub fn vertices(&self) -> wgpu::BufferSlice<'_> {
        self.vertices.slice(..)
    }
}

fn item_ranges(
    len: usize,
    spans: &[super::pack_draw::EntitySpan],
) -> Vec<(std::ops::Range<u32>, Option<i32>)> {
    let mut ranges = Vec::new();
    let first = spans
        .first()
        .map_or(len, |span| (span.start as usize).min(len));
    if first > 0 {
        ranges.push((0..first as u32, None));
    }
    for (span, range) in super::pack_entities::runs(len, spans) {
        if !range.is_empty() {
            ranges.push((range.start as u32..range.end as u32, span.item_id));
        }
    }
    ranges
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn first_person_draws_keep_arms_and_both_items_distinct() {
        use super::super::pack_draw::EntitySpan;
        let spans = [
            EntitySpan::default(),
            EntitySpan {
                start: 4,
                item_id: Some(17),
                ..Default::default()
            },
            EntitySpan {
                start: 12,
                ..Default::default()
            },
            EntitySpan {
                start: 16,
                item_id: Some(29),
                ..Default::default()
            },
            EntitySpan {
                start: 20,
                ..Default::default()
            },
        ];
        assert_eq!(
            item_ranges(24, &spans),
            [
                (0..4, None),
                (4..12, Some(17)),
                (12..16, None),
                (16..20, Some(29)),
                (20..24, None)
            ]
        );
        assert_eq!(item_ranges(8, &[]), [(0..8, None)]);
        assert!(item_ranges(0, &spans).is_empty());
    }

    /// The hand's window depth lies within `MC_HAND_DEPTH` of the middle (packs tell the hand
    /// apart by it), and x, y match the plain projection.
    #[test]
    fn hand_depth_is_scaled_around_the_middle() {
        let p = projection(70.0, 16.0 / 9.0, 512.0);
        let plain = Mat4::perspective_rh_gl(70f32.to_radians(), 16.0 / 9.0, 0.05, 512.0);
        for z in [-0.06f32, -0.5, -3.0, -400.0] {
            let v = glam::Vec4::new(0.1, -0.2, z, 1.0);
            let (a, b) = (p * v, plain * v);
            let window = (a.z / a.w) * 0.5 + 0.5;
            assert!(
                (window - 0.5).abs() <= HAND_DEPTH * 0.5 + 1e-6,
                "{z}: {window}"
            );
            assert!((a.x / a.w - b.x / b.w).abs() < 1e-6);
        }
    }
}
