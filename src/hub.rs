use audio::{AudioData, Operation as AudioOperation};
use color::{self, Color};
use light::{ShadowMap, ShadowProjection};
use material::{self, Material};
use mesh::DynamicMesh;
use node::{NodeInternal, NodePointer, TransformInternal};
use object;
use render::GpuData;
use text::{Operation as TextOperation, TextData};

use cgmath::Transform;
use froggy;
use mint;

use std::mem;
use std::sync::{Arc, Mutex};
use std::sync::mpsc;


#[derive(Clone, Debug)]
pub(crate) enum SubLight {
    Ambient,
    Directional,
    Hemisphere { ground: Color },
    Point,
}

#[derive(Clone, Debug)]
pub(crate) struct LightData {
    pub(crate) color: Color,
    pub(crate) intensity: f32,
    pub(crate) sub_light: SubLight,
    pub(crate) shadow: Option<(ShadowMap, ShadowProjection)>,
}

/// A sub-node specifies and contains the context-specific data owned by a `Node`.
#[derive(Debug)]
pub(crate) enum SubNode {
    /// No extra data.
    Empty,
    /// Group can be a parent to other objects.
    Group { first_child: Option<NodePointer> },
    /// Audio data.
    Audio(AudioData),
    /// Renderable text for 2D user interface.
    UiText(TextData),
    /// Renderable 3D content, such as a mesh.
    Visual(Material, GpuData),
    /// Lighting information for illumination and shadow casting.
    Light(LightData),
}

pub(crate) type Message = (froggy::WeakPointer<NodeInternal>, Operation);
pub(crate) enum Operation {
    AddChild(NodePointer),
    SetAudio(AudioOperation),
    SetVisible(bool),
    SetText(TextOperation),
    SetTransform(
        Option<mint::Point3<f32>>,
        Option<mint::Quaternion<f32>>,
        Option<f32>,
    ),
    SetMaterial(Material),
    SetTexelRange(mint::Point2<i16>, mint::Vector2<u16>),
    SetShadow(ShadowMap, ShadowProjection),
}

pub(crate) type HubPtr = Arc<Mutex<Hub>>;

pub(crate) struct Hub {
    pub(crate) nodes: froggy::Storage<NodeInternal>,
    pub(crate) message_tx: mpsc::Sender<Message>,
    message_rx: mpsc::Receiver<Message>,
}

impl Hub {
    pub(crate) fn new() -> HubPtr {
        let (tx, rx) = mpsc::channel();
        let hub = Hub {
            nodes: froggy::Storage::new(),
            message_tx: tx,
            message_rx: rx,
        };
        Arc::new(Mutex::new(hub))
    }

    // !!TODO: convert to indexing?!!
    pub(crate) fn get<T>(
        &self,
        object: T,
    ) -> &NodeInternal
    where
        T: AsRef<object::Base>,
    {
        let base: &object::Base = object.as_ref();
        &self.nodes[&base.node]
    }

    pub(crate) fn get_mut<T>(
        &mut self,
        object: T,
    ) -> &mut NodeInternal
    where
        T: AsRef<object::Base>,
    {
        let base: &object::Base = object.as_ref();
        &mut self.nodes[&base.node]
    }

    fn spawn(
        &mut self,
        sub: SubNode,
    ) -> object::Base {
        object::Base {
            node: self.nodes.create(sub.into()),
            tx: self.message_tx.clone(),
        }
    }

    pub(crate) fn spawn_empty(&mut self) -> object::Base {
        self.spawn(SubNode::Empty)
    }

    pub(crate) fn spawn_group(&mut self) -> object::Base {
        self.spawn(SubNode::Group { first_child: None })
    }

    pub(crate) fn spawn_visual(
        &mut self,
        mat: Material,
        gpu_data: GpuData,
    ) -> object::Base {
        self.spawn(SubNode::Visual(mat, gpu_data))
    }

    pub(crate) fn spawn_light(
        &mut self,
        data: LightData,
    ) -> object::Base {
        self.spawn(SubNode::Light(data))
    }

    pub(crate) fn spawn_ui_text(
        &mut self,
        text: TextData,
    ) -> object::Base {
        self.spawn(SubNode::UiText(text))
    }

    pub(crate) fn spawn_audio_source(
        &mut self,
        data: AudioData,
    ) -> object::Base {
        self.spawn(SubNode::Audio(data))
    }

    pub(crate) fn process_messages(&mut self) {
        let mut deferred_sibling_updates = Vec::new();

        while let Ok((pnode, operation)) = self.message_rx.try_recv() {
            let node = match pnode.upgrade() {
                Ok(ptr) => &mut self.nodes[&ptr],
                Err(_) => continue,
            };
            match operation {
                Operation::AddChild(child_ptr) => if let SubNode::Group { ref mut first_child } = node.sub_node {
                    let sibling = mem::replace(first_child, Some(child_ptr.clone()));
                    deferred_sibling_updates.push((child_ptr, sibling));
                }
                Operation::SetAudio(operation) => if let SubNode::Audio(ref mut data) = node.sub_node {
                    Hub::process_audio(operation, data);
                },
                Operation::SetVisible(visible) => {
                    node.visible = visible;
                }
                Operation::SetTransform(pos, rot, scale) => {
                    if let Some(pos) = pos {
                        node.transform.disp = mint::Vector3::from(pos).into();
                    }
                    if let Some(rot) = rot {
                        node.transform.rot = rot.into();
                    }
                    if let Some(scale) = scale {
                        node.transform.scale = scale;
                    }
                }
                Operation::SetMaterial(material) => if let SubNode::Visual(ref mut mat, _) = node.sub_node {
                    *mat = material;
                },
                Operation::SetTexelRange(base, size) => if let SubNode::Visual(ref mut material, _) = node.sub_node {
                    match *material {
                        material::Material::Sprite(ref mut params) => params.map.set_texel_range(base, size),
                        _ => panic!("Unsupported material for texel range request"),
                    }
                },
                Operation::SetText(operation) => if let SubNode::UiText(ref mut data) = node.sub_node {
                    Hub::process_text(operation, data);
                },
                Operation::SetShadow(map, proj) => if let SubNode::Light(ref mut data) = node.sub_node {
                    data.shadow = Some((map, proj));
                },
            }
        }

        for (child_ptr, sibling) in deferred_sibling_updates {
            let child = &mut self.nodes[&child_ptr];
            if child.next_sibling.is_some() {
                error!("Attaching a child that still has an old parent, discarding siblings");
            }
            child.next_sibling = sibling;
        }

        self.nodes.sync_pending();
    }

    fn process_audio(
        operation: AudioOperation,
        data: &mut AudioData,
    ) {
        match operation {
            AudioOperation::Append(clip) => data.source.append(clip),
            AudioOperation::Pause => data.source.pause(),
            AudioOperation::Resume => data.source.resume(),
            AudioOperation::Stop => data.source.stop(),
            AudioOperation::SetVolume(volume) => data.source.set_volume(volume),
        }
    }

    fn process_text(
        operation: TextOperation,
        data: &mut TextData,
    ) {
        use gfx_glyph::Scale;
        match operation {
            TextOperation::Color(color) => {
                let rgb = color::to_linear_rgb(color);
                data.section.text[0].color = [rgb[0], rgb[1], rgb[2], 0.0];
            }
            TextOperation::Font(font) => data.font = font,
            TextOperation::Layout(layout) => data.layout = layout,
            TextOperation::Opacity(opacity) => data.section.text[0].color[3] = opacity,
            TextOperation::Pos(point) => data.section.screen_position = (point.x, point.y),
            // TODO: somehow grab window::hdpi_factor and multiply size
            TextOperation::Scale(scale) => data.section.text[0].scale = Scale::uniform(scale),
            TextOperation::Size(size) => data.section.bounds = (size.x, size.y),
            TextOperation::Text(text) => data.section.text[0].text = text,
        }
    }

    pub(crate) fn update_mesh(
        &mut self,
        mesh: &DynamicMesh,
    ) {
        match self.get_mut(&mesh).sub_node {
            SubNode::Visual(_, ref mut gpu_data) => gpu_data.pending = Some(mesh.dynamic.clone()),
            _ => unreachable!(),
        }
    }

    pub(crate) fn walk(&self, base: &Option<NodePointer>) -> TreeWalker {
        let mut walker = TreeWalker {
            hub: self,
            stack: Vec::new(),
            only_visible: true,
        };
        walker.descend(base);
        walker
    }
}

#[derive(Debug)]
pub(crate) struct WalkedNode<'a> {
    pub(crate) world_visible: bool,
    pub(crate) world_transform: TransformInternal,
    pub(crate) node: &'a NodeInternal,
}

pub(crate) struct TreeWalker<'a> {
    hub: &'a Hub,
    stack: Vec<WalkedNode<'a>>,
    only_visible: bool,
}

impl<'a> TreeWalker<'a> {
    fn descend(&mut self, base: &Option<NodePointer>) -> Option<&NodeInternal> {
        let mut node = &self.hub.nodes[base.as_ref()?];

        loop {
            let wn = match self.stack.last() {
                Some(parent) => WalkedNode {
                    world_visible: parent.world_visible && node.visible,
                    world_transform: parent.world_transform.concat(&node.transform),
                    node,
                },
                None => WalkedNode {
                    world_visible: node.visible,
                    world_transform: node.transform,
                    node,
                },
            };
            self.stack.push(wn);

            if self.only_visible && !node.visible {
                break;
            }

            node = match node.sub_node {
                SubNode::Group { first_child: Some(ref ptr) } => &self.hub.nodes[ptr],
                _ => break,
            };
        }

        Some(node)
    }
}

impl<'a> Iterator for TreeWalker<'a> {
    type Item = WalkedNode<'a>;

    fn next(&mut self) -> Option<Self::Item> {
        while let Some(top) = self.stack.pop() {
            self.descend(&top.node.next_sibling);
            if !self.only_visible || top.world_visible {
                return Some(top)
            }
        }
        None
    }
}
