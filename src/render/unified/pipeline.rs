use bevy::ecs::query::ROQueryItem;
use bevy::ecs::system::{SystemParam, SystemParamItem};
use bevy::prelude::{Commands, Mesh, Rect, Resource, Vec3, With};
use bevy::render::globals::{GlobalsBuffer, GlobalsUniform};
use bevy::render::mesh::VertexAttributeValues;
use bevy::render::render_phase::{
    DrawFunctionId, PhaseItem, RenderCommand, RenderCommandResult, SetItemPipeline,
};
use bevy::render::render_resource::{
    CachedRenderPipelineId, DynamicUniformBuffer, ShaderType, SpecializedRenderPipeline,
    SpecializedRenderPipelines,
};
use bevy::render::view::ViewTarget;
use bevy::utils::FloatOrd;
use bevy::{
    ecs::system::lifetimeless::{Read, SRes},
    math::{Mat4, Quat, Vec2, Vec4},
    prelude::{Component, Entity, FromWorld, Handle, Query, Res, ResMut, World},
    render::{
        color::Color,
        render_asset::RenderAssets,
        render_phase::{DrawFunctions, TrackedRenderPass},
        render_resource::{
            BindGroup, BindGroupEntry, BindGroupLayout, BindGroupLayoutDescriptor,
            BindGroupLayoutEntry, BindingResource, BindingType, BlendState, BufferBindingType,
            BufferSize, BufferUsages, BufferVec, ColorTargetState, ColorWrites, Extent3d,
            FragmentState, FrontFace, MultisampleState, PipelineCache, PolygonMode, PrimitiveState,
            PrimitiveTopology, RenderPipelineDescriptor, SamplerBindingType, SamplerDescriptor,
            ShaderStages, TextureDescriptor, TextureDimension, TextureFormat, TextureSampleType,
            TextureUsages, TextureViewDescriptor, TextureViewDimension, VertexAttribute,
            VertexBufferLayout, VertexFormat, VertexState, VertexStepMode,
        },
        renderer::{RenderDevice, RenderQueue},
        texture::{BevyDefault, GpuImage, Image},
    },
    utils::HashMap,
};
use bevy_svg::prelude::Svg;
use bytemuck::{Pod, Zeroable};
use kayak_font::{bevy::FontTextureCache, KayakFont};
use std::marker::PhantomData;

use super::UNIFIED_SHADER_HANDLE;
use crate::layout::LayoutCache;
use crate::prelude::Corner;
use crate::render::extract::{UIExtractedView, UIViewUniform, UIViewUniformOffset, UIViewUniforms};
use crate::render::opacity_layer::OpacityLayerManager;
use crate::render::svg::RenderSvgs;
use crate::render::ui_pass::{
    TransparentOpacityUI, TransparentUI, TransparentUIGeneric, UIRenderPhase,
};

#[derive(Resource, Clone)]
pub struct UnifiedPipeline {
    pub view_layout: BindGroupLayout,
    pub types_layout: BindGroupLayout,
    pub image_layout: BindGroupLayout,
    empty_font_texture: GpuImage,
    default_image: (GpuImage, BindGroup),
}

// const QUAD_VERTEX_POSITIONS: &[Vec3] = &[
//     Vec3::from_array([0.0, 1.0, 0.0]),
//     Vec3::from_array([1.0, 0.0, 0.0]),
//     Vec3::from_array([0.0, 0.0, 0.0]),
//     Vec3::from_array([0.0, 1.0, 0.0]),
//     Vec3::from_array([1.0, 1.0, 0.0]),
//     Vec3::from_array([1.0, 0.0, 0.0]),
// ];

const QUAD_INDICES: [usize; 6] = [0, 2, 3, 0, 1, 2];

const QUAD_VERTEX_POSITIONS: [Vec2; 4] = [
    Vec2::new(0.0, 0.0),
    Vec2::new(1.0, 0.0),
    Vec2::new(1.0, 1.0),
    Vec2::new(0.0, 1.0),
];

#[derive(Debug, Component, Clone, Copy, PartialEq, Eq, Hash)]
pub struct UnifiedPipelineKey {
    pub msaa: u32,
    pub hdr: bool,
}

impl FromWorld for UnifiedPipeline {
    fn from_world(world: &mut World) -> Self {
        let world = world.cell();
        let render_device = world.resource::<RenderDevice>();

        let view_layout = render_device.create_bind_group_layout(&BindGroupLayoutDescriptor {
            entries: &[
                BindGroupLayoutEntry {
                    binding: 0,
                    visibility: ShaderStages::VERTEX | ShaderStages::FRAGMENT,
                    ty: BindingType::Buffer {
                        ty: BufferBindingType::Uniform,
                        has_dynamic_offset: true,
                        min_binding_size: Some(UIViewUniform::min_size()),
                    },
                    count: None,
                },
                BindGroupLayoutEntry {
                    binding: 1,
                    visibility: ShaderStages::VERTEX_FRAGMENT,
                    ty: BindingType::Buffer {
                        ty: BufferBindingType::Uniform,
                        has_dynamic_offset: false,
                        min_binding_size: Some(GlobalsUniform::min_size()),
                    },
                    count: None,
                },
            ],
            label: Some("ui_view_layout"),
        });

        let types_layout = render_device.create_bind_group_layout(&BindGroupLayoutDescriptor {
            entries: &[BindGroupLayoutEntry {
                binding: 0,
                visibility: ShaderStages::VERTEX | ShaderStages::FRAGMENT,
                ty: BindingType::Buffer {
                    ty: BufferBindingType::Uniform,
                    has_dynamic_offset: true,
                    // TODO: change this to ViewUniform::std140_size_static once crevice fixes this!
                    // Context: https://github.com/LPGhatguy/crevice/issues/29
                    min_binding_size: BufferSize::new(16),
                },
                count: None,
            }],
            label: Some("ui_types_layout"),
        });

        let image_layout = render_device.create_bind_group_layout(&BindGroupLayoutDescriptor {
            entries: &[
                BindGroupLayoutEntry {
                    binding: 0,
                    visibility: ShaderStages::FRAGMENT,
                    ty: BindingType::Texture {
                        multisampled: false,
                        sample_type: TextureSampleType::Float { filterable: true },
                        view_dimension: TextureViewDimension::D2,
                    },
                    count: None,
                },
                BindGroupLayoutEntry {
                    binding: 1,
                    visibility: ShaderStages::FRAGMENT,
                    ty: BindingType::Sampler(SamplerBindingType::Filtering),
                    count: None,
                },
                BindGroupLayoutEntry {
                    binding: 2,
                    visibility: ShaderStages::FRAGMENT,
                    ty: BindingType::Texture {
                        multisampled: false,
                        sample_type: TextureSampleType::Float { filterable: true },
                        view_dimension: TextureViewDimension::D2Array,
                    },
                    count: None,
                },
                BindGroupLayoutEntry {
                    binding: 3,
                    visibility: ShaderStages::FRAGMENT,
                    ty: BindingType::Sampler(SamplerBindingType::Filtering),
                    count: None,
                },
            ],
            label: Some("image_layout"),
        });

        let empty_font_texture = FontTextureCache::get_empty(&render_device);

        let texture_descriptor = TextureDescriptor {
            label: Some("empty_texture"),
            size: Extent3d {
                width: 1,
                height: 1,
                depth_or_array_layers: 1,
            },
            mip_level_count: 1,
            sample_count: 1,
            dimension: TextureDimension::D2,
            view_formats: &[TextureFormat::Rgba8UnormSrgb],
            format: TextureFormat::Rgba8UnormSrgb,
            usage: TextureUsages::TEXTURE_BINDING | TextureUsages::COPY_DST,
        };

        let sampler_descriptor = SamplerDescriptor::default();

        let texture = render_device.create_texture(&texture_descriptor);
        let sampler = render_device.create_sampler(&sampler_descriptor);

        let texture_view = texture.create_view(&TextureViewDescriptor {
            label: Some("empty_texture_view"),
            format: Some(TextureFormat::Rgba8UnormSrgb),
            dimension: Some(TextureViewDimension::D2),
            aspect: bevy::render::render_resource::TextureAspect::All,
            base_mip_level: 0,
            base_array_layer: 0,
            mip_level_count: None,
            array_layer_count: None,
        });

        let image = GpuImage {
            texture,
            sampler,
            texture_view,
            mip_level_count: 1,
            size: Vec2::new(1.0, 1.0),
            texture_format: TextureFormat::Rgba8UnormSrgb,
        };

        let binding = render_device.create_bind_group(
            Some("default_image_bind_group"),
            &image_layout,
            &[
                BindGroupEntry {
                    binding: 0,
                    resource: BindingResource::TextureView(&image.texture_view),
                },
                BindGroupEntry {
                    binding: 1,
                    resource: BindingResource::Sampler(&image.sampler),
                },
                BindGroupEntry {
                    binding: 2,
                    resource: BindingResource::TextureView(&empty_font_texture.texture_view),
                },
                BindGroupEntry {
                    binding: 3,
                    resource: BindingResource::Sampler(&empty_font_texture.sampler),
                },
            ],
        );

        UnifiedPipeline {
            view_layout,
            empty_font_texture,
            types_layout,
            image_layout,
            default_image: (image, binding),
        }
    }
}

impl SpecializedRenderPipeline for UnifiedPipeline {
    type Key = UnifiedPipelineKey;

    fn specialize(&self, key: Self::Key) -> RenderPipelineDescriptor {
        let vertex_buffer_layout = VertexBufferLayout {
            array_stride: 60,
            step_mode: VertexStepMode::Vertex,
            attributes: vec![
                VertexAttribute {
                    format: VertexFormat::Float32x3,
                    offset: 0,
                    shader_location: 0,
                },
                VertexAttribute {
                    format: VertexFormat::Float32x4,
                    offset: 12,
                    shader_location: 1,
                },
                VertexAttribute {
                    format: VertexFormat::Float32x4,
                    offset: 28,
                    shader_location: 2,
                },
                VertexAttribute {
                    format: VertexFormat::Float32x4,
                    offset: 44,
                    shader_location: 3,
                },
            ],
        };

        RenderPipelineDescriptor {
            vertex: VertexState {
                shader: UNIFIED_SHADER_HANDLE,
                entry_point: "vertex".into(),
                shader_defs: vec![],
                buffers: vec![vertex_buffer_layout],
            },
            fragment: Some(FragmentState {
                shader: UNIFIED_SHADER_HANDLE,
                shader_defs: vec![],
                entry_point: "fragment".into(),
                targets: vec![Some(ColorTargetState {
                    format: if key.hdr {
                        ViewTarget::TEXTURE_FORMAT_HDR
                    } else {
                        TextureFormat::bevy_default()
                    },
                    blend: Some(BlendState::ALPHA_BLENDING),
                    // Some(BlendState {
                    //     color: BlendComponent {
                    //         src_factor: BlendFactor::SrcAlpha,
                    //         dst_factor: BlendFactor::OneMinusSrcAlpha,
                    //         operation: BlendOperation::Add,
                    //     },
                    //     alpha: BlendComponent {
                    //         src_factor: BlendFactor::OneMinusDstAlpha,
                    //         dst_factor: BlendFactor::One,
                    //         operation: BlendOperation::Add,
                    //     },
                    // }),
                    write_mask: ColorWrites::ALL,
                })],
            }),
            layout: vec![
                self.view_layout.clone(),
                self.image_layout.clone(),
                self.types_layout.clone(),
            ],
            primitive: PrimitiveState {
                front_face: FrontFace::Ccw,
                cull_mode: None,
                polygon_mode: PolygonMode::Fill,
                conservative: false,
                topology: PrimitiveTopology::TriangleList,
                strip_index_format: None,
                unclipped_depth: false,
            },
            depth_stencil: None,
            multisample: MultisampleState {
                count: key.msaa,
                mask: !0,
                alpha_to_coverage_enabled: false,
            },
            label: Some("unified_pipeline".into()),
            push_constant_ranges: vec![],
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd)]
pub enum UIQuadType {
    Quad,
    BoxShadow,
    Text,
    TextSubpixel,
    Image,
    Clip,
    OpacityLayer,
    DrawOpacityLayer,
    None,
}

impl UIQuadType {
    pub fn get_type_index(&self, quad_type_offsets: &QuadTypeOffsets) -> u32 {
        match self {
            UIQuadType::Quad => quad_type_offsets.quad_type_offset,
            UIQuadType::Text => quad_type_offsets.text_type_offset,
            UIQuadType::TextSubpixel => quad_type_offsets.text_sub_pixel_type_offset,
            UIQuadType::Image => quad_type_offsets.image_type_offset,
            UIQuadType::BoxShadow => quad_type_offsets.box_shadow_type_offset,
            UIQuadType::Clip => 100000,
            UIQuadType::None => 100001,
            UIQuadType::OpacityLayer => 100002,
            UIQuadType::DrawOpacityLayer => quad_type_offsets.image_type_offset,
        }
    }
}

#[derive(Debug, Component, Clone)]
pub struct ExtractedQuad {
    pub org_entity: Entity,
    pub camera_entity: Entity,
    pub rect: Rect,
    pub color: Color,
    pub char_id: u32,
    pub z_index: f32,
    pub font_handle: Option<Handle<KayakFont>>,
    pub quad_type: UIQuadType,
    pub type_index: u32,
    pub border_radius: Corner<f32>,
    pub image: Option<Handle<Image>>,
    pub uv_min: Option<Vec2>,
    pub uv_max: Option<Vec2>,
    pub svg_handle: (Option<Handle<Svg>>, Option<Color>),
    pub opacity_layer: u32,
    pub c: char,
}

impl Default for ExtractedQuad {
    fn default() -> Self {
        Self {
            org_entity: Entity::PLACEHOLDER,
            camera_entity: Entity::PLACEHOLDER,
            rect: Default::default(),
            color: Default::default(),
            char_id: Default::default(),
            z_index: Default::default(),
            font_handle: Default::default(),
            quad_type: UIQuadType::Quad,
            type_index: Default::default(),
            border_radius: Default::default(),
            image: Default::default(),
            uv_min: Default::default(),
            uv_max: Default::default(),
            svg_handle: Default::default(),
            opacity_layer: 0,
            c: ' ',
        }
    }
}

#[repr(C)]
#[derive(Copy, Clone)]
pub struct QuadVertex {
    pub position: [f32; 3],
    pub color: [f32; 4],
    pub uv: [f32; 4],
    pub pos_size: [f32; 4],
}

unsafe impl Zeroable for QuadVertex {}
unsafe impl Pod for QuadVertex {}

#[repr(C)]
#[derive(Copy, Clone, ShaderType)]
struct QuadType {
    pub t: i32,
    pub _padding_1: i32,
    pub _padding_2: i32,
    pub _padding_3: i32,
}

#[derive(Resource)]
pub struct QuadMeta {
    pub vertices: BufferVec<QuadVertex>,
    types_buffer: DynamicUniformBuffer<QuadType>,
    types_bind_group: Option<BindGroup>,
}

impl Default for QuadMeta {
    fn default() -> Self {
        Self {
            vertices: BufferVec::new(BufferUsages::VERTEX),
            types_buffer: DynamicUniformBuffer::default(),
            types_bind_group: None,
        }
    }
}

#[derive(Debug)]
pub enum QuadOrMaterial {
    Quad(ExtractedQuad),
    Material(Entity),
}

impl QuadOrMaterial {
    pub fn get_entity(&self) -> Entity {
        match self {
            QuadOrMaterial::Material(entity) => *entity,
            QuadOrMaterial::Quad(quad) => quad.org_entity,
        }
    }
}

impl Default for QuadOrMaterial {
    fn default() -> Self {
        Self::Quad(ExtractedQuad::default())
    }
}

#[derive(Resource, Default, Debug)]
pub struct ExtractedQuads {
    layers: Vec<ZLayer>,
    current_layer: usize,
    current_index: usize,
    children: HashMap<usize, Vec<usize>>,
    parents: HashMap<usize, usize>,
}

impl ExtractedQuads {
    pub fn clear(&mut self) {
        self.current_index = 0;
        self.current_layer = 0;
        self.layers.clear();
        self.children.clear();
        self.parents.clear();
    }

    pub fn push(&mut self, quad: QuadOrMaterial) {
        let layer = self.layers.get_mut(self.current_layer).unwrap();
        layer.quads.push(quad);
    }

    pub fn extend(&mut self, quads: Vec<QuadOrMaterial>) {
        let layer = self.layers.get_mut(self.current_layer).unwrap();
        layer.quads.extend(quads);
    }
    pub fn new_layer(&mut self, z_index: Option<f32>) {
        let layer = ZLayer {
            custom_z: z_index.unwrap_or(0.0),
            parent_id: self.current_layer,
            ..Default::default()
        };

        self.current_index = self.layers.len();
        if let Some(c) = self.children.get_mut(&self.current_layer) {
            c.push(self.current_index);
        }
        self.parents.insert(self.current_index, self.current_layer);
        self.layers.push(layer);
        self.current_layer = self.current_index;
        self.children.insert(self.current_index, vec![]);
    }

    pub fn pop_stack(&mut self) {
        let layer = self.layers.get_mut(self.current_layer).unwrap();
        self.current_layer = layer.parent_id;
    }

    pub(crate) fn resolve(&mut self, commands: &mut Commands, layout_cache: &mut LayoutCache) {
        let mut stack = vec![0];

        #[allow(clippy::manual_while_let_some)]
        while !stack.is_empty() {
            let layer_id = stack.pop().unwrap();
            let parent_id = self.layers.get(layer_id).map(|l| l.parent_id).unwrap();
            let parent_z = {
                self.layers
                    .get(parent_id)
                    .map(|l| l.custom_z)
                    .unwrap_or(0.0)
            };
            let children = self.children.get(&layer_id).cloned().unwrap_or_default();
            stack.extend(children);
            let layer = &mut self.layers[layer_id];
            let quad_count = layer.quads.len();
            layer.z = (layer_id.max(parent_id) as f32) + parent_z + layer.custom_z;
            layer.custom_z = if layer.custom_z > 0.0 {
                layer.custom_z
            } else {
                parent_z
            };
            for (i, quad) in layer.quads.iter_mut().enumerate() {
                let current_z = layer.z + (((i + 1) as f32 / quad_count as f32) * 0.999);
                match quad {
                    QuadOrMaterial::Material(entity) => {
                        commands.entity(*entity).insert(MaterialZ(current_z));
                    }
                    QuadOrMaterial::Quad(quad) => {
                        quad.z_index = current_z;
                        if quad.org_entity != Entity::PLACEHOLDER {
                            if let Some(layout) = layout_cache
                                .rect
                                .get_mut(&crate::node::WrappedIndex(quad.org_entity))
                            {
                                layout.z_index = Some(quad.z_index);
                            }
                        }
                    }
                }

                // Propagate z up the tree until we hit a parent with a Z value set.
                // let mut has_z = false;
                // let mut current_node = WrappedIndex(quad.get_entity());
                // while !has_z {
                //     if let Some(parent) = tree.get_parent(current_node) {
                //         if let Some(layout) = layout_cache.rect.get_mut(&parent) {
                //             if layout.z_index.is_none() {
                //                 layout.z_index = Some(current_z);
                //                 current_node = parent;
                //                 continue;
                //             }
                //         }
                //     }
                //     has_z = true;
                // }
            }
        }
    }

    pub fn debug(&self) {
        let mut items = vec![];
        let mut stack = vec![0];
        #[allow(clippy::manual_while_let_some)]
        while !stack.is_empty() {
            let layer_id = stack.pop().unwrap();
            let parent_id = self.layers.get(layer_id).map(|l| l.parent_id).unwrap_or(0);
            // let parent_z = {
            //     self.layers
            //         .get(self.parents.get(&parent_id).map(|id| *id).unwrap_or(0))
            //         .map(|l| l.z)
            //         .unwrap_or(0.0)
            // };
            let children = self.children.get(&layer_id).cloned().unwrap_or_default();
            stack.extend(children);
            let layer = &self.layers[layer_id];

            let qt = layer
                .quads
                .first()
                .map(|q| match q {
                    QuadOrMaterial::Quad(q) => q.quad_type,
                    _ => UIQuadType::None,
                })
                .unwrap_or(UIQuadType::None);
            // let rect = layer
            //     .quads
            //     .first()
            //     .map(|q| match q {
            //         QuadOrMaterial::Quad(q) => q.rect,
            //         _ => Rect::default(),
            //     })
            //     .unwrap_or(Rect::default());
            if qt != UIQuadType::None {
                // items.push((layer.z, format!("{}type: {:?}, layer_id: {}, parent_id: {}, parent_z: {}, z: {}, rect: {:?}", " ".repeat(parent_id + 1), qt, layer_id, parent_id, parent_z, layer.z, rect)));
            }
            if !layer.quads.is_empty() {
                // println!("{}Quads:", " ".repeat(parent_id + 1));
                let mut last_type = UIQuadType::None;
                for quad in layer.quads.iter() {
                    #[allow(clippy::single_match)]
                    match quad {
                        QuadOrMaterial::Quad(q) => {
                            if last_type != q.quad_type {
                                items.push((
                                    q.z_index,
                                    format!(
                                        "{}Q: {:?}, c: {}, rect: {:?}, z: {}",
                                        " ".repeat(parent_id + 1),
                                        q.quad_type,
                                        q.c,
                                        q.rect,
                                        q.z_index
                                    ),
                                ));
                                last_type = q.quad_type;
                            }
                        }
                        _ => {}
                    }
                }
            }
        }

        items.sort_by(|a, b| a.0.partial_cmp(&b.0).unwrap());

        for (_, item) in items.iter() {
            println!("{}", item);
        }
    }

    pub fn iter(&self) -> impl Iterator<Item = &ExtractedQuad> + '_ {
        self.layers
            .iter()
            .flat_map(|layer| &layer.quads)
            .filter_map(|quad| match &quad {
                QuadOrMaterial::Material(_) => None,
                QuadOrMaterial::Quad(quad) => match quad.quad_type {
                    UIQuadType::None => None,
                    _ => Some(quad),
                },
            })
    }
}

#[derive(Component)]
pub struct MaterialZ(pub f32);

#[derive(Default, Debug)]
pub struct ZLayer {
    pub z: f32,
    pub custom_z: f32,
    pub parent_id: usize,
    pub quads: Vec<QuadOrMaterial>,
}

#[derive(Debug, Component, PartialEq, Clone)]
pub struct QuadBatch {
    pub image_handle_id: Option<Handle<Image>>,
    pub font_handle_id: Option<Handle<KayakFont>>,
    pub quad_type: UIQuadType,
    pub type_id: u32,
    pub z_index: f32,
}

#[derive(Default, Resource)]
pub struct ImageBindGroups {
    values: HashMap<Handle<Image>, BindGroup>,
    font_values: HashMap<Handle<KayakFont>, BindGroup>,
    previous_sizes: HashMap<Handle<Image>, Vec2>,
}

#[derive(Component, Debug)]
pub struct UIViewBindGroup {
    pub value: BindGroup,
}

pub fn queue_ui_view_bind_groups(
    mut commands: Commands,
    render_device: Res<RenderDevice>,
    unified_pipeline: Res<UnifiedPipeline>,
    view_uniforms: Res<UIViewUniforms>,
    views: Query<Entity, With<UIExtractedView>>,
    globals_buffer: Res<GlobalsBuffer>,
) {
    if let (Some(view_binding), Some(globals)) = (
        view_uniforms.uniforms.binding(),
        globals_buffer.buffer.binding(),
    ) {
        for entity in &views {
            let view_bind_group = render_device.create_bind_group(
                Some("ui_view_bind_group"),
                &unified_pipeline.view_layout,
                &[
                    BindGroupEntry {
                        binding: 0,
                        resource: view_binding.clone(),
                    },
                    BindGroupEntry {
                        binding: 1,
                        resource: globals.clone(),
                    },
                ],
            );
            commands.entity(entity).insert(UIViewBindGroup {
                value: view_bind_group,
            });
        }
    }
}

#[derive(Resource, Default, Debug, Clone, Copy)]
pub struct QuadTypeOffsets {
    pub quad_type_offset: u32,
    pub text_sub_pixel_type_offset: u32,
    pub text_type_offset: u32,
    pub image_type_offset: u32,
    pub box_shadow_type_offset: u32,
}

pub fn queue_quad_types(
    mut commands: Commands,
    quad_pipeline: Res<UnifiedPipeline>,
    render_device: Res<RenderDevice>,
    render_queue: Res<RenderQueue>,
    mut quad_meta: ResMut<QuadMeta>,
) {
    quad_meta.types_buffer.clear();
    // sprite_meta.types_buffer.reserve(2, &render_device);
    let quad_type_offset = quad_meta.types_buffer.push(QuadType {
        t: 0,
        _padding_1: 0,
        _padding_2: 0,
        _padding_3: 0,
    });
    let text_sub_pixel_type_offset = quad_meta.types_buffer.push(QuadType {
        t: 1,
        _padding_1: 0,
        _padding_2: 0,
        _padding_3: 0,
    });
    let text_type_offset = quad_meta.types_buffer.push(QuadType {
        t: 2,
        _padding_1: 0,
        _padding_2: 0,
        _padding_3: 0,
    });
    let image_type_offset = quad_meta.types_buffer.push(QuadType {
        t: 3,
        _padding_1: 0,
        _padding_2: 0,
        _padding_3: 0,
    });
    let box_shadow_type_offset = quad_meta.types_buffer.push(QuadType {
        t: 4,
        _padding_1: 0,
        _padding_2: 0,
        _padding_3: 0,
    });
    let quad_type_offsets = QuadTypeOffsets {
        quad_type_offset,
        text_sub_pixel_type_offset,
        text_type_offset,
        image_type_offset,
        box_shadow_type_offset,
    };
    commands.insert_resource(quad_type_offsets);

    quad_meta
        .types_buffer
        .write_buffer(&render_device, &render_queue);

    if let Some(type_binding) = quad_meta.types_buffer.binding() {
        quad_meta.types_bind_group = Some(render_device.create_bind_group(
            Some("quad_type_bind_group"),
            &quad_pipeline.types_layout,
            &[BindGroupEntry {
                binding: 0,
                resource: type_binding,
            }],
        ));
    }
}

#[derive(Resource, Default)]
pub struct PreviousClip {
    pub rect: Rect,
}

#[derive(Resource, Default)]
pub struct PreviousIndex {
    pub index: u32,
    pub last_clip: Rect,
}

#[derive(SystemParam)]
pub struct QueueQuads<'w, 's> {
    render_svgs: Res<'w, RenderSvgs>,
    opacity_layers: Res<'w, OpacityLayerManager>,
    commands: Commands<'w, 's>,
    draw_functions: Res<'w, DrawFunctions<TransparentUI>>,
    draw_functions_opacity: Res<'w, DrawFunctions<TransparentOpacityUI>>,
    render_device: Res<'w, RenderDevice>,
    render_queue: Res<'w, RenderQueue>,
    quad_meta: ResMut<'w, QuadMeta>,
    quad_pipeline: Res<'w, UnifiedPipeline>,
    pipelines: ResMut<'w, SpecializedRenderPipelines<UnifiedPipeline>>,
    pipeline_cache: Res<'w, PipelineCache>,
    extracted_quads: Res<'w, ExtractedQuads>,
    views: Query<
        'w,
        's,
        (
            Entity,
            &'static mut UIRenderPhase<TransparentUI>,
            &'static mut UIRenderPhase<TransparentOpacityUI>,
            &'static UIExtractedView,
        ),
    >,
    image_bind_groups: ResMut<'w, ImageBindGroups>,
    unified_pipeline: Res<'w, UnifiedPipeline>,
    gpu_images: Res<'w, RenderAssets<Image>>,
    font_texture_cache: Res<'w, FontTextureCache>,
    quad_type_offsets: Res<'w, QuadTypeOffsets>,
    prev_clip: ResMut<'w, PreviousClip>,
    prev_index: ResMut<'w, PreviousIndex>,
    view_uniforms: Res<'w, UIViewUniforms>,
}

pub fn queue_quads(queue_quads: QueueQuads) {
    let QueueQuads {
        render_svgs,
        opacity_layers,
        mut commands,
        draw_functions,
        draw_functions_opacity,
        render_device,
        render_queue,
        mut quad_meta,
        quad_pipeline,
        mut pipelines,
        pipeline_cache,
        extracted_quads,
        mut views,
        mut image_bind_groups,
        unified_pipeline,
        gpu_images,
        font_texture_cache,
        quad_type_offsets,
        mut prev_clip,
        mut prev_index,
        view_uniforms,
    } = queue_quads;

    if view_uniforms.uniforms.buffer().is_none() {
        return;
    }

    let mut extracted_quads = extracted_quads.iter().collect::<Vec<_>>();

    extracted_quads.sort_unstable_by(|a, b| a.z_index.partial_cmp(&b.z_index).unwrap());

    // let mut last_type = UIQuadType::None;
    // for (t, z, rect) in extracted_quads.iter().map(|q| (q.quad_type, q.z_index, q.rect)) {
    //     if !(t == UIQuadType::Text && last_type == UIQuadType::Text) {
    //         println!("type: {:?}, z: {}, rect: {:?}", t, z, rect);
    //         last_type = t;
    //     }
    // }

    let extracted_sprite_len = extracted_quads.len();
    // don't create buffers when there are no quads
    if extracted_sprite_len == 0 {
        return;
    }

    quad_meta.vertices.clear();
    quad_meta.vertices.reserve(
        extracted_sprite_len * QUAD_VERTEX_POSITIONS.len(),
        &render_device,
    );

    // Sort sprites by z for correct transparency and then by handle to improve batching
    // NOTE: This can be done independent of views by reasonably assuming that all 2D views look along the negative-z axis in world space
    let mut current_batch = QuadBatch {
        image_handle_id: None,
        font_handle_id: None,
        quad_type: UIQuadType::None,
        type_id: quad_type_offsets.quad_type_offset,
        z_index: 0.0,
    };
    let mut current_batch_entity = Entity::PLACEHOLDER;

    // Vertex buffer indices
    let mut index = 0;
    let mut item_start = 0;
    let mut item_end = 0;
    let mut old_item_start = 0;
    let mut current_clip = Rect::default();
    let mut last_clip = Rect::default();

    let draw_quad = draw_functions.read().get_id::<DrawUI>().unwrap();
    let draw_opacity_quad = draw_functions_opacity
        .read()
        .get_id::<DrawUITransparent>()
        .unwrap();
    for (camera_entity, mut transparent_phase, mut opacity_transparent_phase, view) in
        views.iter_mut()
    {
        let key = UnifiedPipelineKey {
            msaa: 1,
            hdr: view.hdr,
        };
        let spec_pipeline = pipelines.specialize(&pipeline_cache, &quad_pipeline, key);

        let mut last_quad = ExtractedQuad::default();

        for quad in extracted_quads.iter() {
            if quad.quad_type == UIQuadType::Clip {
                prev_clip.rect = quad.rect;
            }

            queue_quads_inner(
                &mut commands,
                &render_device,
                &font_texture_cache,
                &opacity_layers,
                &mut image_bind_groups,
                &gpu_images,
                &unified_pipeline,
                &render_svgs,
                &mut transparent_phase,
                &mut opacity_transparent_phase,
                draw_opacity_quad,
                draw_quad,
                spec_pipeline,
                &mut quad_meta,
                quad,
                camera_entity,
                *quad_type_offsets,
                &mut current_batch,
                &mut current_batch_entity,
                &mut index,
                &mut item_start,
                &mut item_end,
                &last_quad,
                &mut current_clip,
                &mut old_item_start,
                &mut last_clip,
            );

            last_quad = (*quad).clone();
        }

        #[allow(clippy::nonminimal_bool)]
        if last_quad.quad_type != UIQuadType::Clip
            && last_quad.quad_type != UIQuadType::OpacityLayer
            && last_quad.quad_type != UIQuadType::Clip
            && current_batch_entity != Entity::PLACEHOLDER
        {
            commands
                .entity(current_batch_entity)
                .insert(current_batch.clone());

            if last_quad.opacity_layer > 0 && last_quad.quad_type != UIQuadType::DrawOpacityLayer {
                opacity_transparent_phase.add(TransparentOpacityUI {
                    draw_function: draw_opacity_quad,
                    pipeline: spec_pipeline,
                    entity: current_batch_entity,
                    sort_key: FloatOrd(last_quad.z_index),
                    quad_type: last_quad.quad_type,
                    type_index: last_quad.quad_type.get_type_index(&quad_type_offsets),
                    rect: last_clip,
                    batch_range: Some(old_item_start..item_end),
                    opacity_layer: last_quad.opacity_layer,
                    dynamic_offset: None,
                });
            } else {
                transparent_phase.add(TransparentUI {
                    draw_function: draw_quad,
                    pipeline: spec_pipeline,
                    entity: current_batch_entity,
                    sort_key: FloatOrd(last_quad.z_index),
                    quad_type: last_quad.quad_type,
                    type_index: last_quad.quad_type.get_type_index(&quad_type_offsets),
                    rect: last_clip,
                    batch_range: Some(old_item_start..item_end),
                    dynamic_offset: None,
                });
            }
        }
    }

    quad_meta
        .vertices
        .write_buffer(&render_device, &render_queue);

    prev_index.index = index;
    prev_index.last_clip = last_clip;
}

pub fn queue_quads_inner(
    commands: &mut Commands,
    render_device: &RenderDevice,
    font_texture_cache: &FontTextureCache,
    opacity_layers: &OpacityLayerManager,
    image_bind_groups: &mut ImageBindGroups,
    gpu_images: &RenderAssets<Image>,
    unified_pipeline: &UnifiedPipeline,
    render_svgs: &RenderSvgs,
    transparent_phase: &mut UIRenderPhase<TransparentUI>,
    opacity_transparent_phase: &mut UIRenderPhase<TransparentOpacityUI>,
    draw_opacity_quad: DrawFunctionId,
    draw_quad: DrawFunctionId,
    spec_pipeline: CachedRenderPipelineId,
    quad_meta: &mut QuadMeta,
    quad: &ExtractedQuad,
    camera_entity: Entity,
    quad_type_offsets: QuadTypeOffsets,
    current_batch: &mut QuadBatch,
    current_batch_entity: &mut Entity,
    index: &mut u32,
    item_start: &mut u32,
    item_end: &mut u32,
    old_quad: &ExtractedQuad,
    current_clip: &mut Rect,
    old_item_start: &mut u32,
    last_clip: &mut Rect,
) {
    if camera_entity != quad.camera_entity {
        return;
    }

    // Ignore opacity layers
    if quad.quad_type == UIQuadType::OpacityLayer || quad.quad_type == UIQuadType::None {
        return;
    }

    if (current_clip.width() < 1.0 || current_clip.height() < 1.0)
        && quad.quad_type != UIQuadType::Clip
    {
        return;
    }

    if quad.quad_type == UIQuadType::Clip {
        // *last_clip = *current_clip;
        *current_clip = quad.rect;
    }

    let mut new_batch = QuadBatch {
        image_handle_id: quad.image.clone(),
        font_handle_id: quad.font_handle.clone(),
        quad_type: quad.quad_type,
        type_id: quad.quad_type.get_type_index(&quad_type_offsets),
        z_index: quad.z_index,
        // z_index: 0.0,
    };
    let sprite_rect = quad.rect;

    if (new_batch != *current_batch || current_batch.quad_type != quad.quad_type)
        || old_quad.quad_type == UIQuadType::Clip
        || quad.quad_type == UIQuadType::Clip
        || matches!(new_batch.quad_type, UIQuadType::DrawOpacityLayer)
    {
        if *current_batch_entity != Entity::PLACEHOLDER
            && old_quad.quad_type != UIQuadType::Clip
            && old_quad.quad_type != UIQuadType::OpacityLayer
        {
            // handle old batch
            commands
                .entity(*current_batch_entity)
                .insert(current_batch.clone());
        }

        // batch ended insert transparent phase object:
        if current_batch.quad_type != UIQuadType::Clip
            && current_batch.quad_type != UIQuadType::OpacityLayer
            && old_quad.quad_type != UIQuadType::Clip
            && *current_batch_entity != Entity::PLACEHOLDER
        {
            if old_quad.opacity_layer > 0 && old_quad.quad_type != UIQuadType::DrawOpacityLayer {
                opacity_transparent_phase.add(TransparentOpacityUI {
                    draw_function: draw_opacity_quad,
                    pipeline: spec_pipeline,
                    entity: *current_batch_entity,
                    sort_key: FloatOrd(old_quad.z_index),
                    quad_type: old_quad.quad_type,
                    type_index: current_batch.type_id,
                    rect: *current_clip,
                    batch_range: Some(*old_item_start..*item_end),
                    opacity_layer: old_quad.opacity_layer,
                    dynamic_offset: None,
                });
            } else {
                transparent_phase.add(TransparentUI {
                    draw_function: draw_quad,
                    pipeline: spec_pipeline,
                    entity: *current_batch_entity,
                    sort_key: FloatOrd(old_quad.z_index),
                    quad_type: old_quad.quad_type,
                    type_index: current_batch.type_id,
                    rect: *last_clip,
                    batch_range: Some(*old_item_start..*item_end),
                    dynamic_offset: None,
                });
            }

            *item_start = *index;
            *old_item_start = *item_end;
        }

        if let Some(image_handle) = quad.image.as_ref() {
            if let Some(gpu_image) = gpu_images.get(image_handle) {
                image_bind_groups
                    .values
                    .entry(image_handle.clone_weak())
                    .or_insert_with(|| {
                        render_device.create_bind_group(
                            Some("ui_image_bind_group"),
                            &unified_pipeline.image_layout,
                            &[
                                BindGroupEntry {
                                    binding: 0,
                                    resource: BindingResource::TextureView(&gpu_image.texture_view),
                                },
                                BindGroupEntry {
                                    binding: 1,
                                    resource: BindingResource::Sampler(&gpu_image.sampler),
                                },
                                BindGroupEntry {
                                    binding: 2,
                                    resource: BindingResource::TextureView(
                                        &unified_pipeline.empty_font_texture.texture_view,
                                    ),
                                },
                                BindGroupEntry {
                                    binding: 3,
                                    resource: BindingResource::Sampler(
                                        &unified_pipeline.empty_font_texture.sampler,
                                    ),
                                },
                            ],
                        )
                    });
            } else {
                // Skip unloaded texture.
                return;
            }
        }

        if let Some(font_handle) = quad.font_handle.as_ref() {
            if let Some(gpu_image) = font_texture_cache.get_gpu_image(font_handle, gpu_images) {
                new_batch.font_handle_id = Some(font_handle.clone_weak());
                image_bind_groups
                    .font_values
                    .entry(font_handle.clone_weak())
                    .or_insert_with(|| {
                        render_device.create_bind_group(
                            Some("ui_text_bind_group"),
                            &unified_pipeline.image_layout,
                            &[
                                BindGroupEntry {
                                    binding: 0,
                                    resource: BindingResource::TextureView(
                                        &unified_pipeline.default_image.0.texture_view,
                                    ),
                                },
                                BindGroupEntry {
                                    binding: 1,
                                    resource: BindingResource::Sampler(
                                        &unified_pipeline.default_image.0.sampler,
                                    ),
                                },
                                BindGroupEntry {
                                    binding: 2,
                                    resource: BindingResource::TextureView(&gpu_image.texture_view),
                                },
                                BindGroupEntry {
                                    binding: 3,
                                    resource: BindingResource::Sampler(&gpu_image.sampler),
                                },
                            ],
                        )
                    });
            }
        }

        if quad.quad_type == UIQuadType::DrawOpacityLayer {
            if let Some(layer) = opacity_layers.camera_layers.get(&camera_entity) {
                let image_handle = layer.get_image_handle(quad.opacity_layer);
                if let Some(gpu_image) = gpu_images.get(&image_handle) {
                    let new_image = if let Some(prev_size) =
                        image_bind_groups.previous_sizes.get(&image_handle)
                    {
                        if gpu_image.size != *prev_size {
                            image_bind_groups
                                .previous_sizes
                                .insert(image_handle.clone_weak(), gpu_image.size);
                            true
                        } else {
                            false
                        }
                    } else {
                        image_bind_groups
                            .previous_sizes
                            .insert(image_handle.clone_weak(), gpu_image.size);
                        true
                    };

                    if new_image {
                        image_bind_groups.values.insert(
                            image_handle.clone_weak(),
                            render_device.create_bind_group(
                                Some("draw_opacity_layer_bind_group"),
                                &unified_pipeline.image_layout,
                                &[
                                    BindGroupEntry {
                                        binding: 0,
                                        resource: BindingResource::TextureView(
                                            &gpu_image.texture_view,
                                        ),
                                    },
                                    BindGroupEntry {
                                        binding: 1,
                                        resource: BindingResource::Sampler(&gpu_image.sampler),
                                    },
                                    BindGroupEntry {
                                        binding: 2,
                                        resource: BindingResource::TextureView(
                                            &unified_pipeline.empty_font_texture.texture_view,
                                        ),
                                    },
                                    BindGroupEntry {
                                        binding: 3,
                                        resource: BindingResource::Sampler(
                                            &unified_pipeline.empty_font_texture.sampler,
                                        ),
                                    },
                                ],
                            ),
                        );
                    }

                    new_batch.image_handle_id = Some(image_handle.clone_weak());
                    // bevy::prelude::info!("Attaching opacity layer with index: {} with view: {:?}", quad.opacity_layer, gpu_image.texture_view);
                } else {
                    return;
                }
            }
        }

        // Start new batch
        *current_batch = new_batch;
        *last_clip = *current_clip;
        if current_batch.quad_type != UIQuadType::Clip
            && current_batch.quad_type != UIQuadType::OpacityLayer
        {
            *current_batch_entity = commands.spawn(current_batch.clone()).id();
        }
    }

    if matches!(current_batch.quad_type, UIQuadType::Clip) {
        return;
    }

    if let (Some(svg_handle), color) = (quad.svg_handle.0.as_ref(), quad.svg_handle.1.as_ref()) {
        if let Some((svg, mesh)) = render_svgs.get(&svg_handle.id()) {
            let new_height = (svg.view_box.h as f32 / svg.view_box.w as f32) * sprite_rect.size().x;
            let svg_scale_x = sprite_rect.size().x / svg.view_box.w as f32;
            let svg_scale_y = new_height / svg.view_box.h as f32;
            let positions = mesh
                .attribute(Mesh::ATTRIBUTE_POSITION)
                .unwrap()
                .as_float3()
                .unwrap();
            let colors = match mesh.attribute(Mesh::ATTRIBUTE_COLOR).unwrap() {
                VertexAttributeValues::Float32x4(d) => Some(d),
                _ => None,
            }
            .unwrap();
            let indices = mesh.indices().unwrap();

            for index in indices.iter() {
                let position = positions[index];
                let color = if let Some(color) = color {
                    [color.r(), color.g(), color.b(), color.a()]
                } else {
                    colors[index]
                };
                let world = Mat4::from_scale_rotation_translation(
                    Vec3::new(svg_scale_x, svg_scale_y, 1.0), //sprite_rect.size().extend(1.0),
                    Quat::default(),
                    sprite_rect.min.extend(0.0),
                );
                let final_position = (world
                    * Vec4::new(
                        position[0],  // - 34.5,
                        -position[1], // - 95.0,
                        position[2],
                        1.0,
                    ))
                .truncate();

                quad_meta.vertices.push(QuadVertex {
                    position: final_position.into(),
                    color,
                    uv: [0.0; 4],
                    pos_size: [
                        sprite_rect.min.x,
                        sprite_rect.min.y,
                        sprite_rect.size().x,
                        new_height,
                    ],
                });
            }
            *index += indices.len() as u32;
            *item_end = *index;
        }
    } else {
        let color = quad.color.as_linear_rgba_f32();

        let uv_min = quad.uv_min.unwrap_or(Vec2::ZERO);
        let uv_max = quad.uv_max.unwrap_or(Vec2::ONE);

        let bottom_left = Vec4::new(
            uv_min.x,
            uv_min.y,
            quad.char_id as f32,
            quad.border_radius.bottom_left,
        );
        let top_left = Vec4::new(
            uv_min.x,
            uv_max.y,
            quad.char_id as f32,
            quad.border_radius.top_left,
        );
        let top_right = Vec4::new(
            uv_max.x,
            uv_max.y,
            quad.char_id as f32,
            quad.border_radius.top_right,
        );
        let bottom_right = Vec4::new(
            uv_max.x,
            uv_min.y,
            quad.char_id as f32,
            quad.border_radius.bottom_right,
        );

        let uvs: [[f32; 4]; 6] = [
            top_left.into(),
            bottom_right.into(),
            bottom_left.into(),
            top_left.into(),
            top_right.into(),
            bottom_right.into(),
        ];

        const QUAD_INDICES: [usize; 6] = [0, 2, 3, 0, 1, 2];

        const QUAD_VERTEX_POSITIONS: [Vec2; 4] = [
            Vec2::new(0.0, 0.0),
            Vec2::new(1.0, 0.0),
            Vec2::new(1.0, 1.0),
            Vec2::new(0.0, 1.0),
        ];

        for (index, vertex_index) in QUAD_INDICES.iter().enumerate() {
            let vertex_position = QUAD_VERTEX_POSITIONS[*vertex_index];
            let world = Mat4::from_scale_rotation_translation(
                sprite_rect.size().extend(1.0),
                Quat::default(),
                sprite_rect.min.extend(0.0),
            );
            let final_position = (world * vertex_position.extend(0.0).extend(1.0)).truncate();
            quad_meta.vertices.push(QuadVertex {
                position: final_position.into(),
                color,
                uv: uvs[index],
                pos_size: [
                    sprite_rect.min.x,
                    sprite_rect.min.y,
                    sprite_rect.size().x,
                    sprite_rect.size().y,
                ],
            });
        }

        *index += QUAD_INDICES.len() as u32;
        *item_end = *index;
    }
}

pub type DrawUI = (
    SetItemPipeline,
    SetUIViewBindGroup<TransparentUI, 0>,
    DrawUIDraw<TransparentUI>,
);

pub type DrawUITransparent = (
    SetItemPipeline,
    SetUIViewBindGroup<TransparentOpacityUI, 0>,
    DrawUIDraw<TransparentOpacityUI>,
);

pub struct SetUIViewBindGroup<T, const I: usize> {
    phantom: PhantomData<T>,
}
impl<T: PhaseItem, const I: usize> RenderCommand<T> for SetUIViewBindGroup<T, I> {
    type Param = ();
    type ViewWorldQuery = (Read<UIViewUniformOffset>, Read<UIViewBindGroup>);
    type ItemWorldQuery = ();

    #[inline]
    fn render<'w>(
        _item: &T,
        (view_uniform, ui_view_bind_group): ROQueryItem<'w, Self::ViewWorldQuery>,
        _: (),
        _: SystemParamItem<'w, '_, Self::Param>,
        pass: &mut TrackedRenderPass<'w>,
    ) -> RenderCommandResult {
        pass.set_bind_group(I, &ui_view_bind_group.value, &[view_uniform.offset]);
        RenderCommandResult::Success
    }
}

#[derive(Default)]
pub struct DrawUIDraw<T> {
    phantom: PhantomData<T>,
}

impl<T: PhaseItem + TransparentUIGeneric> RenderCommand<T> for DrawUIDraw<T> {
    type Param = (SRes<QuadMeta>, SRes<UnifiedPipeline>, SRes<ImageBindGroups>);

    type ViewWorldQuery = Read<UIExtractedView>;
    type ItemWorldQuery = Read<QuadBatch>;

    fn render<'w>(
        item: &T,
        view: bevy::ecs::query::ROQueryItem<'w, Self::ViewWorldQuery>,
        batch: bevy::ecs::query::ROQueryItem<'w, Self::ItemWorldQuery>,
        param: bevy::ecs::system::SystemParamItem<'w, '_, Self::Param>,
        pass: &mut TrackedRenderPass<'w>,
    ) -> RenderCommandResult {
        let (quad_meta, unified_pipeline, image_bind_groups) = param;

        let quad_meta = quad_meta.into_inner();
        let window_size = (view.viewport.z as f32, view.viewport.w as f32);
        let rect = item.get_rect();
        let x = rect.min.x as u32;
        let y = rect.min.y as u32;
        let mut width = rect.width() as u32;
        let mut height = rect.height() as u32;

        width = width.min(window_size.0 as u32);
        height = height.min(window_size.1 as u32);
        if !(width == 0 || height == 0 || x > window_size.0 as u32 || y > window_size.1 as u32) {
            if x + width >= window_size.0 as u32 {
                width = window_size.0 as u32 - x;
            }
            if y + height >= window_size.1 as u32 {
                height = window_size.1 as u32 - y;
            }
            pass.set_scissor_rect(x, y, width, height);
        }

        let vertices_slice = quad_meta.vertices.buffer().unwrap().slice(..);
        pass.set_vertex_buffer(0, vertices_slice);

        pass.set_bind_group(
            2,
            quad_meta.types_bind_group.as_ref().unwrap(),
            &[item.get_type_index()],
        );

        let unified_pipeline = unified_pipeline.into_inner();

        let image_bind_groups = image_bind_groups.into_inner();
        if let Some(image_handle) = batch.image_handle_id.as_ref() {
            if let Some(bind_group) = image_bind_groups.values.get(image_handle) {
                pass.set_bind_group(1, bind_group, &[]);
            } else {
                pass.set_bind_group(1, &unified_pipeline.default_image.1, &[]);
            }
        } else if let Some(bind_group) = batch
            .font_handle_id
            .as_ref()
            .and_then(|h| image_bind_groups.font_values.get(h))
        {
            pass.set_bind_group(1, bind_group, &[]);
        } else {
            pass.set_bind_group(1, &unified_pipeline.default_image.1, &[]);
        }
        pass.draw(item.batch_range().clone(), 0..1);

        RenderCommandResult::Success
    }
}
