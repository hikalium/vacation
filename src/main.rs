#![feature(slice_flatten)]

use anyhow::anyhow;
use anyhow::Context;
use anyhow::Result;
use argh::FromArgs;
use gltf::Node;
use gltf::Semantic;
use gltf_json as json;
use json::validation::Checked::Valid;
use std::borrow::Cow;
use std::fs;
use std::io;
use std::mem;

#[derive(Copy, Clone, Debug)]
#[repr(C)]
struct Vertex {
    position: [f32; 3],
    color: [f32; 3],
}

#[derive(FromArgs)]
/// VRM as a Code
struct Args {
    /// path to .vrm file to parse
    #[argh(option)]
    input: Option<String>,
    /// path to .vrm file to export
    #[argh(option)]
    output: Option<String>,
}

fn parse_node(node: &Node, depth: usize) -> Result<()> {
    print!("{:width$}", "", width = depth);
    println!("name: {:?}", node.name());
    for c in node.children() {
        parse_node(&c, depth + 1)?;
    }
    Ok(())
}

fn run_input(path: &str) -> Result<()> {
    let file = fs::File::open(&path)?;
    let reader = io::BufReader::new(file);
    let gltf = gltf::Gltf::from_reader(reader)?;

    let file = fs::File::open(&path)?;
    let reader = io::BufReader::new(file);
    let bin = gltf::binary::Glb::from_reader(reader)?
        .bin
        .context("No binary section")?;
    println!("BIN section has {} bytes", bin.len());

    for scene in gltf.scenes() {
        println!(
            "Scene #{} has {} children",
            scene.index(),
            scene.nodes().count(),
        );
        for node in scene.nodes() {
            parse_node(&node, 0)?;
        }
    }
    for mesh in gltf.meshes() {
        println!(
            "Mesh #{} has {} primitives. name = {:?}",
            mesh.index(),
            mesh.primitives().count(),
            mesh.name()
        );
        for p in mesh.primitives() {
            println!(
                "primitive #{}: Mode = {:?}, BB = {:?}",
                p.index(),
                p.mode(),
                p.bounding_box(),
            );
            if let (Some(ap), Some(ai)) = (p.get(&Semantic::Positions), p.indices()) {
                println!(
                    "Positions accessor: {:?} {:?} {}",
                    ap.dimensions(),
                    ap.data_type(),
                    ap.count(),
                );
                println!(
                    "Indices accessor: {:?} {:?} {}",
                    ai.dimensions(),
                    ai.data_type(),
                    ai.count(),
                );
                if let Some(v) = ap.view() {
                    println!(
                        "View: len {} bytes, ofs {} bytes, stride: {:?}, name: {:?}",
                        v.length(),
                        v.offset(),
                        v.stride(),
                        v.name()
                    );
                    let b = v.buffer();
                    println!(
                        "Buf #{}: from {:?}, name {:?}, len {} bytes",
                        b.index(),
                        b.source(),
                        b.name(),
                        b.length()
                    );
                    if v.length() < 1000 {
                        let data = &bin[v.offset()..(v.offset() + v.length())];
                        let data: Vec<f32> = data
                            .chunks_exact(4)
                            .map(|ve| {
                                let mut vec = [0u8; 4];
                                vec.copy_from_slice(ve);
                                f32::from_le_bytes(vec)
                            })
                            .collect();
                        let data: Vec<[f32; 3]> =
                            data.chunks_exact(3).map(|v| [v[0], v[1], v[2]]).collect();
                        println!("{} triangles", data.len());
                        for v in data {
                            println!("{:?}", v)
                        }
                    }
                }
            }
        }
    }
    Ok(())
}

/// Calculate bounding coordinates of a list of vertices, used for the clipping distance of the model
fn bounding_coords(points: &[Vertex]) -> ([f32; 3], [f32; 3]) {
    let mut min = [f32::MAX, f32::MAX, f32::MAX];
    let mut max = [f32::MIN, f32::MIN, f32::MIN];

    for point in points {
        let p = point.position;
        for i in 0..3 {
            min[i] = f32::min(min[i], p[i]);
            max[i] = f32::max(max[i], p[i]);
        }
    }
    (min, max)
}

fn align_to_multiple_of_four(n: &mut u32) {
    *n = (*n + 3) & !3;
}

fn append_bytes<T>(bin: &mut Vec<u8>, src: &[T]) -> (u32, u32) {
    let ofs = bin.len();
    assert_eq!(ofs % 4, 0);
    let src: &[u8] = unsafe {
        std::slice::from_raw_parts(
            src.as_ptr() as *const u8,
            src.len() * std::mem::size_of::<T>(),
        )
    };
    let len = src.len();
    // Append the data
    bin.resize(bin.len() + len, 0);
    bin[ofs..ofs + len].copy_from_slice(&src[0..len]);
    // Insert padding if needed
    while bin.len() % 4 != 0 {
        bin.push(0); // pad to multiple of four bytes
    }
    eprintln!("append_bytes: added {} bytes at ofs {}", len, ofs);
    (ofs as u32, len as u32)
}
fn run_output(path: &str) -> Result<()> {
    let triangle_vertices = vec![
        Vertex {
            position: [0.0, 0.5, 0.0],
            color: [1.0, 0.0, 0.0],
        },
        Vertex {
            position: [-0.5, -0.5, 0.0],
            color: [0.0, 1.0, 0.0],
        },
        Vertex {
            position: [0.5, -0.5, 0.0],
            color: [0.0, 0.0, 1.0],
        },
        Vertex {
            position: [0.0, 0.0, 1.0],
            color: [0.0, 0.0, 1.0],
        },
    ];
    let mut bin = Vec::new();
    let (bin_vertices_ofs, bin_vertices_len) = append_bytes(&mut bin, &triangle_vertices);
    let indices: Vec<[u32; 3]> = vec![[0, 1, 2], [1, 2, 3]];
    let indices = indices.flatten();
    let (bin_indices_ofs, bin_indices_len) = append_bytes(&mut bin, indices);

    let bin_size = bin.len() as u32;
    let buffer = json::Buffer {
        byte_length: bin_size,
        extensions: Default::default(),
        extras: Default::default(),
        name: None,
        uri: None,
    };

    let vertex_buffer_view = json::buffer::View {
        buffer: json::Index::new(0),
        byte_length: bin_vertices_len,
        byte_offset: Some(bin_vertices_ofs),
        byte_stride: Some(mem::size_of::<Vertex>() as u32),
        extensions: Default::default(),
        extras: Default::default(),
        name: None,
        target: Some(Valid(json::buffer::Target::ArrayBuffer)),
    };
    let indices_buffer_view = json::buffer::View {
        buffer: json::Index::new(0),
        byte_length: bin_indices_len,
        byte_offset: Some(bin_indices_ofs),
        byte_stride: None,
        extensions: Default::default(),
        extras: Default::default(),
        name: None,
        target: Some(Valid(json::buffer::Target::ArrayBuffer)),
    };

    let (min, max) = bounding_coords(&triangle_vertices);
    let positions = json::Accessor {
        buffer_view: Some(json::Index::new(0)),
        byte_offset: 0,
        count: triangle_vertices.len() as u32,
        component_type: Valid(json::accessor::GenericComponentType(
            json::accessor::ComponentType::F32,
        )),
        extensions: Default::default(),
        extras: Default::default(),
        type_: Valid(json::accessor::Type::Vec3),
        min: Some(json::Value::from(Vec::from(min))),
        max: Some(json::Value::from(Vec::from(max))),
        name: None,
        normalized: false,
        sparse: None,
    };
    let colors = json::Accessor {
        buffer_view: Some(json::Index::new(0)),
        byte_offset: (3 * mem::size_of::<f32>()) as u32,
        count: triangle_vertices.len() as u32,
        component_type: Valid(json::accessor::GenericComponentType(
            json::accessor::ComponentType::F32,
        )),
        extensions: Default::default(),
        extras: Default::default(),
        type_: Valid(json::accessor::Type::Vec3),
        min: None,
        max: None,
        name: None,
        normalized: false,
        sparse: None,
    };

    let indices = json::Accessor {
        buffer_view: Some(json::Index::new(1)),
        byte_offset: 0,
        count: indices.len() as u32,
        component_type: Valid(json::accessor::GenericComponentType(
            json::accessor::ComponentType::U32,
        )),
        extensions: Default::default(),
        extras: Default::default(),
        type_: Valid(json::accessor::Type::Scalar),
        min: None,
        max: None,
        name: None,
        normalized: false,
        sparse: None,
    };

    let primitive = json::mesh::Primitive {
        attributes: {
            let mut map = std::collections::HashMap::new();
            map.insert(Valid(json::mesh::Semantic::Positions), json::Index::new(0));
            map.insert(Valid(json::mesh::Semantic::Colors(0)), json::Index::new(1));
            map
        },
        extensions: Default::default(),
        extras: Default::default(),
        indices: Some(json::Index::new(2)),
        material: None,
        mode: Valid(json::mesh::Mode::Triangles),
        targets: None,
    };

    let mesh = json::Mesh {
        extensions: Default::default(),
        extras: Default::default(),
        name: None,
        primitives: vec![primitive],
        weights: None,
    };

    let node = json::Node {
        camera: None,
        children: None,
        extensions: Default::default(),
        extras: Default::default(),
        matrix: None,
        mesh: Some(json::Index::new(0)),
        name: None,
        rotation: None,
        scale: None,
        translation: None,
        skin: None,
        weights: None,
    };

    let root = json::Root {
        accessors: vec![positions, colors, indices],
        buffers: vec![buffer],
        buffer_views: vec![vertex_buffer_view, indices_buffer_view],
        meshes: vec![mesh],
        nodes: vec![node],
        scenes: vec![json::Scene {
            extensions: Default::default(),
            extras: Default::default(),
            name: None,
            nodes: vec![json::Index::new(0)],
        }],
        ..Default::default()
    };

    let json_string = json::serialize::to_string(&root).expect("Serialization error");
    let mut json_offset = json_string.len() as u32;
    align_to_multiple_of_four(&mut json_offset);
    let glb = gltf::binary::Glb {
        header: gltf::binary::Header {
            magic: *b"glTF",
            version: 2,
            length: json_offset + bin_size,
        },
        bin: Some(Cow::Owned(bin)),
        json: Cow::Owned(json_string.into_bytes()),
    };
    let writer = std::fs::File::create(path).expect("I/O error");
    glb.to_writer(writer).expect("glTF binary output error");

    Ok(())
}
fn main() -> Result<()> {
    let args: Args = argh::from_env();
    if let Some(path) = args.input {
        run_input(&path)
    } else if let Some(path) = args.output {
        run_output(&path)
    } else {
        Err(anyhow!("Run vacation --help for more information."))
    }
}
