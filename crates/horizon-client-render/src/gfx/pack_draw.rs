//! The uniforms a shader pack's gbuffers programs read per draw (`HzDraw` at set 2, the
//! translator's `DRAW_UNIFORMS`): one slot per render stage of a uniform buffer, picked with a
//! dynamic offset at each draw.

use horizon_shaders::{RENDER_STAGES, render_stage};

/// Where a run of `Frame::entities` begins, and what it is (slice 7): the run lasts until the
/// next span. Recorded only while a shader pack draws entities, so Horizon's own frames carry
/// none; Horizon itself still draws the whole list at once.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct EntitySpan {
    /// The first vertex.
    pub start: u32,
    /// The render stage (5.4: `ENTITIES`, `BLOCK_ENTITIES`, `PARTICLES`, `DESTROY`), which picks
    /// the program.
    pub stage: u32,
    /// `entityId` for entities, `blockEntityId` for block entities (5.3), else 0.
    pub id: i32,
    /// `currentRenderedItemId` of an item layer, independent of its entity id.
    /// None for bodies, arms and other geometry that is not an item.
    pub item_id: Option<i32>,
    /// An entity part that blends (a name tag, a shadow): `gbuffers_entities_translucent`.
    pub translucent: bool,
    /// Hidden from the camera (Horizon's entity culling, the player in first person): only the
    /// shadow pass draws it.
    pub hidden: bool,
    /// The local player (`shadowPlayer`).
    pub player: bool,
    /// A block entity whose block emits light (`shadowLightBlockEntities`).
    pub emissive: bool,
}

impl EntitySpan {
    /// Starts an item's run and returns the enclosing entity's identity to
    /// restore after it. Both CPU vertices and GPU skin jobs use these spans.
    pub fn item(spans: &mut Vec<Self>, start: u32, item_id: i32) -> Option<Self> {
        let previous = *spans.last()?;
        Self::push(
            spans,
            Self {
                start,
                item_id: Some(item_id),
                ..previous
            },
        );
        Some(previous)
    }

    /// Ends a temporary layer's identity at `start`; an empty layer is replaced.
    pub fn restore(spans: &mut Vec<Self>, start: u32, previous: Option<Self>) {
        if let Some(previous) = previous {
            Self::push(spans, Self { start, ..previous });
        }
    }

    fn push(spans: &mut Vec<Self>, span: Self) {
        match spans.last_mut() {
            Some(last) if last.start == span.start => *last = span,
            _ => spans.push(span),
        }
    }

    /// Drops identities for removed vertices and resets the item layer at the
    /// new end. A later draw may start there without inheriting a culled item.
    pub fn truncate(spans: &mut Vec<Self>, end: u32) {
        let tail = spans.iter().rfind(|span| span.start <= end).copied();
        spans.retain(|span| span.start < end);
        if let Some(tail) = tail {
            let reset = Self {
                start: end,
                item_id: None,
                ..tail
            };
            // Keep an already matching run coalesced; only a changed identity
            // or an active item needs a new reset marker at the cut.
            if spans.last().is_none_or(|last| {
                (Self {
                    start: end,
                    ..*last
                }) != reset
            }) {
                spans.push(reset);
            }
        }
    }

    /// Marks vertices `start..end` of `spans`' list (one entity) as hidden: the span covering
    /// `start` is split there, the spans inside are hidden, and the geometry after `end` goes
    /// on in the last span's kind. False when no span covers `start`.
    pub fn hide(spans: &mut Vec<EntitySpan>, start: usize, end: usize) -> bool {
        let Some(i) = spans.iter().rposition(|s| s.start as usize <= start) else {
            return false;
        };
        let after = spans[spans.len() - 1];
        if spans[i].start as usize != start {
            let split = EntitySpan {
                start: start as u32,
                ..spans[i]
            };
            spans.insert(i + 1, split);
        }
        for s in spans.iter_mut().skip(i) {
            if s.start as usize >= start {
                s.hidden = true;
            }
        }
        if end > start {
            spans.push(EntitySpan {
                start: end as u32,
                ..after
            });
        }
        true
    }
}

/// `HzDraw`'s size: four scalar ids, then `entityColor`.
const SIZE: u32 = 32;

/// The renderer/translator's std140 draw contract. A non-item draw resets the
/// id to -1, so a previous item cannot leak into the following body or arm.
pub(super) fn encode(
    stage: u32,
    entity: i32,
    block: i32,
    item: Option<i32>,
    color: [f32; 4],
) -> [u8; SIZE as usize] {
    let mut bytes = [0; SIZE as usize];
    for (i, value) in [stage as i32, entity, block, item.unwrap_or(-1)]
        .iter()
        .enumerate()
    {
        bytes[i * 4..i * 4 + 4].copy_from_slice(&value.to_le_bytes());
    }
    for (i, value) in color.iter().enumerate() {
        bytes[16 + i * 4..20 + i * 4].copy_from_slice(&value.to_le_bytes());
    }
    bytes
}

pub(super) struct DrawUniforms {
    pub layout: wgpu::BindGroupLayout,
    group: wgpu::BindGroup,
    stride: u32,
}

impl DrawUniforms {
    pub fn new(device: &wgpu::Device) -> Self {
        let stride = SIZE.max(device.limits().min_uniform_buffer_offset_alignment);
        let mut contents = vec![0u8; stride as usize * RENDER_STAGES.len()];
        for (i, slot) in contents.chunks_mut(stride as usize).enumerate() {
            slot[..SIZE as usize].copy_from_slice(&encode(i as u32, 0, -1, None, [0.0; 4]));
        }
        let buffer = wgpu::util::DeviceExt::create_buffer_init(
            device,
            &wgpu::util::BufferInitDescriptor {
                label: Some("Shader pack draws"),
                contents: &contents,
                usage: wgpu::BufferUsages::UNIFORM,
            },
        );
        let layout = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
            label: Some("Shader pack draws"),
            entries: &[wgpu::BindGroupLayoutEntry {
                binding: 0,
                visibility: wgpu::ShaderStages::VERTEX_FRAGMENT,
                ty: wgpu::BindingType::Buffer {
                    ty: wgpu::BufferBindingType::Uniform,
                    has_dynamic_offset: true,
                    min_binding_size: wgpu::BufferSize::new(u64::from(SIZE)),
                },
                count: None,
            }],
        });
        let group = device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: Some("Shader pack draws"),
            layout: &layout,
            entries: &[wgpu::BindGroupEntry {
                binding: 0,
                resource: wgpu::BindingResource::Buffer(wgpu::BufferBinding {
                    buffer: &buffer,
                    offset: 0,
                    size: wgpu::BufferSize::new(u64::from(SIZE)),
                }),
            }],
        });
        Self {
            layout,
            group,
            stride,
        }
    }

    /// Draws that follow read `renderStage` = `MC_RENDER_STAGE_<stage>`, other values zero.
    pub fn bind(&self, pass: &mut wgpu::RenderPass<'_>, stage: &str) {
        pass.set_bind_group(2, &self.group, &[self.stride * render_stage(stage)]);
    }
}

#[cfg(test)]
mod tests {
    use super::{EntitySpan, encode};

    #[test]
    fn item_layers_restore_the_enclosing_entity_identity() {
        let body = EntitySpan {
            id: 42,
            ..Default::default()
        };
        let mut spans = vec![body];
        let previous = EntitySpan::item(&mut spans, 4, 17);
        EntitySpan::restore(&mut spans, 12, previous);
        let previous = EntitySpan::item(&mut spans, 16, 29);
        EntitySpan::restore(&mut spans, 20, previous);
        assert_eq!(
            spans
                .iter()
                .map(|s| (s.start, s.id, s.item_id))
                .collect::<Vec<_>>(),
            [
                (0, 42, None),
                (4, 42, Some(17)),
                (12, 42, None),
                (16, 42, Some(29)),
                (20, 42, None)
            ]
        );
        let previous = EntitySpan::item(&mut spans, 20, 98);
        EntitySpan::restore(&mut spans, 20, previous);
        assert_eq!(
            spans.last().unwrap().item_id,
            None,
            "empty items do not affect following geometry"
        );
    }

    #[test]
    fn draw_bytes_keep_item_and_entity_ids_independent_and_reset_non_items() {
        let color = [0.25, 0.5, 0.75, 1.0];
        let bytes = encode(11, 42, -1, Some(97), color);
        let i32_at = |bytes: &[u8], offset| {
            i32::from_le_bytes(bytes[offset..offset + 4].try_into().unwrap())
        };
        assert_eq!(
            [
                i32_at(&bytes, 0),
                i32_at(&bytes, 4),
                i32_at(&bytes, 8),
                i32_at(&bytes, 12)
            ],
            [11, 42, -1, 97]
        );
        assert_eq!(f32::from_le_bytes(bytes[16..20].try_into().unwrap()), 0.25);
        let arm = encode(16, 0, -1, None, [0.0; 4]);
        assert_eq!(i32_at(&arm, 12), -1);
    }

    #[test]
    fn culled_cpu_items_cannot_leave_unsorted_ids_for_the_next_entity() {
        let mut spans = vec![EntitySpan {
            id: 11,
            ..Default::default()
        }];
        EntitySpan::push(
            &mut spans,
            EntitySpan {
                start: 4,
                id: 22,
                ..Default::default()
            },
        );
        let previous = EntitySpan::item(&mut spans, 4, 71);
        EntitySpan::restore(&mut spans, 12, previous);
        EntitySpan::truncate(&mut spans, 4);
        assert_eq!(
            spans.last().map(|s| (s.start, s.id, s.item_id)),
            Some((4, 22, None))
        );
        EntitySpan::push(
            &mut spans,
            EntitySpan {
                start: 4,
                id: 33,
                ..Default::default()
            },
        );
        let previous = EntitySpan::item(&mut spans, 8, 82);
        EntitySpan::restore(&mut spans, 12, previous);
        let runs = super::super::pack_entities::runs(16, &spans)
            .map(|(span, range)| (range, span.id, span.item_id))
            .collect::<Vec<_>>();
        assert_eq!(
            runs,
            [
                (0..4, 11, None),
                (4..8, 33, None),
                (8..12, 33, Some(82)),
                (12..16, 33, None)
            ]
        );
    }

    fn span(start: u32, id: i32) -> EntitySpan {
        EntitySpan {
            start,
            id,
            ..Default::default()
        }
    }

    #[test]
    fn hide_splits_the_span_around_one_entity() {
        // Two cows in one span: the second (vertices 8..16) is hidden.
        let mut spans = vec![span(0, 7)];
        assert!(EntitySpan::hide(&mut spans, 8, 16));
        let hidden = EntitySpan {
            hidden: true,
            ..span(8, 7)
        };
        assert_eq!(spans, [span(0, 7), hidden, span(16, 7)]);
        // An entity with its own span and a part after it (armour) is hidden whole.
        let mut spans = vec![span(0, 1), span(4, 2), span(6, 3)];
        assert!(EntitySpan::hide(&mut spans, 4, 10));
        assert!(!spans[0].hidden && spans[1].hidden && spans[2].hidden);
        assert_eq!(spans[3], span(10, 3));
        assert!(!EntitySpan::hide(&mut Vec::new(), 0, 4));
    }
}
