#![feature(slice_flatten)]
#![feature(assert_matches)]

use anyhow::anyhow;
use anyhow::Context;
use anyhow::Result;
use argh::FromArgs;
use gltf::buffer::Source;
use gltf::Node;
use gltf::Semantic;
use gltf_json::validation::Checked::Valid;
use std::assert_matches::assert_matches;
use std::borrow::Cow;
use std::fs;
use std::io;
use std::path::Path;

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

    let parts_dir = Path::new(path).with_extension("parts");
    fs::create_dir_all(parts_dir.clone())?;

    for scene in gltf.scenes() {
        println!("Scene #{}", scene.index(),);
        for node in scene.nodes() {
            parse_node(&node, 0)?;
        }
    }
    let mut pcount = 0;
    for mesh in gltf.meshes() {
        println!(" Mesh #{}: name = {:?}", mesh.index(), mesh.name());
        for p in mesh.primitives() {
            assert_eq!(p.mode(), gltf::mesh::Mode::Triangles);
            if let (Some(ap), Some(ai), Some(at0), Some(bct)) = (
                p.get(&Semantic::Positions),
                p.indices(),
                p.get(&Semantic::TexCoords(0)),
                p.material().pbr_metallic_roughness().base_color_texture(),
            ) {
                println!(
                    "Base Color Texture: tex_coord: {}, texture.index: {}",
                    bct.tex_coord(),
                    bct.texture().index()
                );
                let vertices = {
                    assert_eq!(ap.dimensions(), gltf::accessor::Dimensions::Vec3);
                    assert_eq!(ap.data_type(), gltf::accessor::DataType::F32);
                    let v = ap.view().context("Positions have no view")?;
                    assert_eq!(v.stride(), None);
                    let b = v.buffer();
                    assert_matches!(b.source(), gltf::buffer::Source::Bin);
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
                    data
                };

                let indices = {
                    assert_eq!(ai.dimensions(), gltf::accessor::Dimensions::Scalar);
                    assert_eq!(ai.data_type(), gltf::accessor::DataType::U32);
                    let v = ai.view().context("Indices have no view")?;
                    assert_eq!(v.stride(), None);
                    let b = v.buffer();
                    assert_matches!(b.source(), gltf::buffer::Source::Bin);
                    let data = &bin[v.offset()..(v.offset() + v.length())];
                    let data: Vec<u32> = data
                        .chunks_exact(4)
                        .map(|ve| {
                            let mut vec = [0u8; 4];
                            vec.copy_from_slice(ve);
                            u32::from_le_bytes(vec)
                        })
                        .collect();
                    let data: Vec<[u32; 3]> =
                        data.chunks_exact(3).map(|v| [v[0], v[1], v[2]]).collect();
                    data
                };
                println!(
                    "    primitive {}: {} vertices, {} triangles in {:?}",
                    p.index(),
                    vertices.len(),
                    indices.len(),
                    p.bounding_box(),
                );
                let mut path = parts_dir.clone();
                path.push(format!(
                    "{}{}_{}.glb",
                    mesh.name().unwrap_or("None"),
                    mesh.index(),
                    p.index(),
                ));
                let path = path.to_string_lossy();
                write_glb(
                    &vertices,
                    &indices,
                    Some([0f32, 0f32, pcount as f32 / 10.0]),
                    &path,
                )?;
                pcount += 1;
            }
        }
    }
    for t in gltf.textures() {
        println!(" Texture #{}: name = {:?}", t.index(), t.name());
    }
    for m in gltf.images() {
        println!(" Image #{}: name = {:?}", m.index(), m.name());
        if let gltf::image::Source::View { view, mime_type } = m.source() {
            println!("  source_type: {mime_type}",);
            assert_eq!(mime_type, "image/png");
            let buffer = view.buffer();
            assert_matches!(buffer.source(), Source::Bin);
            let offset = view.offset();
            let length = view.length();
            let mut path = parts_dir.clone();
            path.push(format!("i{}_{}.png", m.index(), m.name().unwrap_or("None"),));
            let path = path.to_string_lossy().into_owned();
            fs::write(path, &bin[offset..(offset + length)])?;
        }
    }
    Ok(())
}

/// Calculate bounding coordinates of a list of vertices, used for the clipping distance of the model
fn bounding_coords(points: &[[f32; 3]]) -> ([f32; 3], [f32; 3]) {
    let mut min = [f32::MAX, f32::MAX, f32::MAX];
    let mut max = [f32::MIN, f32::MIN, f32::MIN];

    for p in points {
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
fn write_glb(
    vertices: &[[f32; 3]],
    indices: &[[u32; 3]],
    translation: Option<[f32; 3]>,
    path: &str,
) -> Result<()> {
    eprintln!("Generating {}...", path);
    let mut bin = Vec::new();
    let (bin_vertices_ofs, bin_vertices_len) = append_bytes(&mut bin, &vertices);
    let indices = indices.flatten();
    let (bin_indices_ofs, bin_indices_len) = append_bytes(&mut bin, indices);

    let bin_size = bin.len() as u32;
    let buffer = gltf_json::Buffer {
        byte_length: bin_size,
        extensions: Default::default(),
        extras: Default::default(),
        name: None,
        uri: None,
    };

    let vertex_buffer_view = gltf_json::buffer::View {
        buffer: gltf_json::Index::new(0),
        byte_length: bin_vertices_len,
        byte_offset: Some(bin_vertices_ofs),
        byte_stride: None,
        extensions: Default::default(),
        extras: Default::default(),
        name: None,
        target: Some(Valid(gltf_json::buffer::Target::ArrayBuffer)),
    };
    let indices_buffer_view = gltf_json::buffer::View {
        buffer: gltf_json::Index::new(0),
        byte_length: bin_indices_len,
        byte_offset: Some(bin_indices_ofs),
        byte_stride: None,
        extensions: Default::default(),
        extras: Default::default(),
        name: None,
        target: Some(Valid(gltf_json::buffer::Target::ArrayBuffer)),
    };

    let (min, max) = bounding_coords(vertices);
    let positions = gltf_json::Accessor {
        buffer_view: Some(gltf_json::Index::new(0)),
        byte_offset: 0,
        count: vertices.len() as u32,
        component_type: Valid(gltf_json::accessor::GenericComponentType(
            gltf_json::accessor::ComponentType::F32,
        )),
        extensions: Default::default(),
        extras: Default::default(),
        type_: Valid(gltf_json::accessor::Type::Vec3),
        min: Some(gltf_json::Value::from(Vec::from(min))),
        max: Some(gltf_json::Value::from(Vec::from(max))),
        name: None,
        normalized: false,
        sparse: None,
    };
    let indices = gltf_json::Accessor {
        buffer_view: Some(gltf_json::Index::new(1)),
        byte_offset: 0,
        count: indices.len() as u32,
        component_type: Valid(gltf_json::accessor::GenericComponentType(
            gltf_json::accessor::ComponentType::U32,
        )),
        extensions: Default::default(),
        extras: Default::default(),
        type_: Valid(gltf_json::accessor::Type::Scalar),
        min: None,
        max: None,
        name: None,
        normalized: false,
        sparse: None,
    };

    let primitive = gltf_json::mesh::Primitive {
        attributes: {
            let mut map = std::collections::HashMap::new();
            map.insert(
                Valid(gltf_json::mesh::Semantic::Positions),
                gltf_json::Index::new(0),
            );
            map
        },
        extensions: Default::default(),
        extras: Default::default(),
        indices: Some(gltf_json::Index::new(1)),
        material: None,
        mode: Valid(gltf_json::mesh::Mode::Triangles),
        targets: None,
    };

    let mesh = gltf_json::Mesh {
        extensions: Default::default(),
        extras: Default::default(),
        name: None,
        primitives: vec![primitive],
        weights: None,
    };

    let node = gltf_json::Node {
        camera: None,
        children: None,
        extensions: Default::default(),
        extras: Default::default(),
        matrix: None,
        mesh: Some(gltf_json::Index::new(0)),
        name: None,
        rotation: None,
        scale: None,
        translation,
        skin: None,
        weights: None,
    };

    let root = gltf_json::Root {
        accessors: vec![positions, indices],
        buffers: vec![buffer],
        buffer_views: vec![vertex_buffer_view, indices_buffer_view],
        meshes: vec![mesh],
        nodes: vec![node],
        scenes: vec![gltf_json::Scene {
            extensions: Default::default(),
            extras: Default::default(),
            name: None,
            nodes: vec![gltf_json::Index::new(0)],
        }],
        ..Default::default()
    };

    let json_string = gltf_json::serialize::to_string(&root).expect("Serialization error");
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
    eprintln!("Written to {}", path);
    Ok(())
}
fn run_output(path: &str) -> Result<()> {
    let vertices = vec![
        [0.0, 0.5, 0.0],
        [-0.5, -0.5, 0.0],
        [0.5, -0.5, 0.0],
        [0.0, 0.0, 1.0],
    ];
    let indices: Vec<[u32; 3]> = vec![[0, 1, 2], [1, 2, 3]];
    write_glb(&vertices, &indices, None, path)?;

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
