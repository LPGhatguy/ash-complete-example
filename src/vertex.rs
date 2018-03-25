use std::mem;

use ash::vk;

use cgmath::{Vector2, Vector3};

#[repr(C)]
#[derive(Debug, Clone)]
pub struct Vertex {
    pub position: Vector2<f32>,
    pub color: Vector3<f32>,
}

impl Vertex {
    pub fn get_binding_description() -> vk::VertexInputBindingDescription {
        vk::VertexInputBindingDescription {
            binding: 0,
            stride: mem::size_of::<Vertex>() as u32,
            input_rate: vk::VertexInputRate::Vertex,
        }
    }

    pub fn get_attribute_descriptions() -> Vec<vk::VertexInputAttributeDescription> {
        vec![
            vk::VertexInputAttributeDescription {
                binding: 0,
                location: 0,
                format: vk::Format::R32g32Sfloat,
                offset: offset_of!(Vertex, position) as u32,
            },
            vk::VertexInputAttributeDescription {
                binding: 0,
                location: 1,
                format: vk::Format::R32g32b32Sfloat,
                offset: offset_of!(Vertex, color) as u32,
            },
        ]
    }
}