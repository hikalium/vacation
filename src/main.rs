#![feature(slice_flatten)]
#![feature(assert_matches)]

use anyhow::anyhow;
use anyhow::Context;
use anyhow::Result;
use argh::FromArgs;
use gltf::buffer::Source;
use gltf::Image;
use gltf::Node;
use gltf::Semantic;
use gltf_json::extensions::texture::TextureTransform;
use gltf_json::extensions::texture::TextureTransformOffset;
use gltf_json::extensions::texture::TextureTransformRotation;
use gltf_json::extensions::texture::TextureTransformScale;
use gltf_json::extras::RawValue;
use gltf_json::image::MimeType;
use gltf_json::material::PbrBaseColorFactor;
use gltf_json::validation::Checked::Valid;
use gltf_json::Accessor;
use gltf_json::Index;
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
}

fn parse_node(node: &Node, depth: usize) -> Result<()> {
    print!("{:width$}", "", width = depth);
    println!("name: {:?}", node.name());
    for c in node.children() {
        parse_node(&c, depth + 1)?;
    }
    Ok(())
}

fn extract_png_data_from_image(bin: &[u8], m: &Image) -> Result<Vec<u8>> {
    println!(" Image #{}: name = {:?}", m.index(), m.name());
    if let gltf::image::Source::View { view, mime_type } = m.source() {
        println!("  source_type: {mime_type}",);
        assert_eq!(mime_type, "image/png");
        let buffer = view.buffer();
        assert_matches!(buffer.source(), Source::Bin);
        let offset = view.offset();
        let length = view.length();
        Ok(Vec::from(&bin[offset..(offset + length)]))
    } else {
        Err(anyhow!("Image not found in the Glb"))
    }
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

    println!("extensions_used: {:?}", gltf.extensions_used());
    println!("extensions_required: {:?}", gltf.extensions_required());

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
            assert!(p.get(&Semantic::TexCoords(1)).is_none());
            assert!(p.get(&Semantic::Colors(0)).is_none());
            assert!(p.get(&Semantic::Normals).is_some());
            assert!(p.get(&Semantic::Tangents).is_none());
            assert!(p.get(&Semantic::Joints(0)).is_some());
            assert!(p.get(&Semantic::Joints(1)).is_none());
            assert!(p.get(&Semantic::Weights(0)).is_some());
            assert!(p.get(&Semantic::Weights(1)).is_none());
            if let (Some(ap), Some(an), Some(ai), Some(at0)) = (
                p.get(&Semantic::Positions),
                p.get(&Semantic::Normals),
                p.indices(),
                p.get(&Semantic::TexCoords(0)),
            ) {
                let pbr = p.material().pbr_metallic_roughness();
                println!(
                    "pbr_factors: base: {:?}, metallic: {:?}, roughness: {:?}",
                    pbr.base_color_factor(),
                    pbr.metallic_factor(),
                    pbr.roughness_factor(),
                );
                let bct = pbr.base_color_texture().unwrap();
                println!(
                    "Base Color Texture: tex_coord: {}, texture.index: {}, texture.source.index: {}, {:?}, {:?}, {:?}, {:?}, {:?}",
                    bct.tex_coord(),
                    bct.texture().index(),
                    bct.texture().source().index(),
                    bct.texture().sampler().mag_filter(),
                    bct.texture().sampler().min_filter(),
                    bct.texture().sampler().wrap_s(),
                    bct.texture().sampler().wrap_t(),
                    bct.texture_transform().is_some(),
                );
                assert!(pbr.metallic_roughness_texture().is_none());
                let png_data = extract_png_data_from_image(&bin, &bct.texture().source())
                    .context("Failed to find a png image for a texture")?;
                let tex_coords0 = {
                    assert_eq!(at0.dimensions(), gltf::accessor::Dimensions::Vec2);
                    assert_eq!(at0.data_type(), gltf::accessor::DataType::F32);
                    let v = at0.view().context("TexCoords have no view")?;
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
                    let data: Vec<[f32; 2]> = data.chunks_exact(2).map(|v| [v[0], v[1]]).collect();
                    data
                };
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
                let normals = {
                    assert_eq!(an.dimensions(), gltf::accessor::Dimensions::Vec3);
                    assert_eq!(an.data_type(), gltf::accessor::DataType::F32);
                    let v = an.view().context("Positions have no view")?;
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
                    &normals,
                    Some((&png_data, tex_coords0.as_slice())),
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
        let png_data = extract_png_data_from_image(&bin, &m).context("Failed to get png data")?;
        let mut path = parts_dir.clone();
        path.push(format!("i{}_{}.png", m.index(), m.name().unwrap_or("None"),));
        let path = path.to_string_lossy().into_owned();
        fs::write(path, png_data)?;
    }
    Ok(())
}

/// Calculate bounding coordinates of a list of vertices, used for the clipping distance of the model
fn bounding_coords3d(points: &[[f32; 3]]) -> ([f32; 3], [f32; 3]) {
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
fn bounding_coords2d(points: &[[f32; 2]]) -> ([f32; 2], [f32; 2]) {
    let mut min = [f32::MAX, f32::MAX];
    let mut max = [f32::MIN, f32::MIN];

    for p in points {
        for i in 0..2 {
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
    normals: &[[f32; 3]],
    material: Option<(&[u8], &[[f32; 2]])>,
    translation: Option<[f32; 3]>,
    path: &str,
) -> Result<()> {
    eprintln!("Generating {}...", path);
    let mut bin = Vec::new();
    let (bin_vertices_ofs, bin_vertices_len) = append_bytes(&mut bin, &vertices);
    let (bin_normals_ofs, bin_normals_len) = append_bytes(&mut bin, &normals);
    let indices = indices.flatten();
    let (bin_indices_ofs, bin_indices_len) = append_bytes(&mut bin, &indices);

    //
    // Buffer views
    //
    let mut buffer_views = Vec::new();

    let vertex_buffer_view_idx = gltf_json::Index::new(buffer_views.len() as u32);
    buffer_views.push(gltf_json::buffer::View {
        buffer: gltf_json::Index::new(0),
        byte_length: bin_vertices_len,
        byte_offset: Some(bin_vertices_ofs),
        byte_stride: None,
        extensions: Default::default(),
        extras: Default::default(),
        name: None,
        target: Some(Valid(gltf_json::buffer::Target::ArrayBuffer)),
    });

    let normals_buffer_view_idx = gltf_json::Index::new(buffer_views.len() as u32);
    buffer_views.push(gltf_json::buffer::View {
        buffer: gltf_json::Index::new(0),
        byte_length: bin_normals_len,
        byte_offset: Some(bin_normals_ofs),
        byte_stride: None,
        extensions: Default::default(),
        extras: Default::default(),
        name: None,
        target: Some(Valid(gltf_json::buffer::Target::ArrayBuffer)),
    });

    let indices_buffer_view_idx = gltf_json::Index::new(buffer_views.len() as u32);
    buffer_views.push(gltf_json::buffer::View {
        buffer: gltf_json::Index::new(0),
        byte_length: bin_indices_len,
        byte_offset: Some(bin_indices_ofs),
        byte_stride: None,
        extensions: Default::default(),
        extras: Default::default(),
        name: None,
        target: Some(Valid(gltf_json::buffer::Target::ArrayBuffer)),
    });

    //
    // Accessors
    //
    let mut accessors = Vec::new();

    let (min, max) = bounding_coords3d(vertices);
    let positions_accessor_idx = gltf_json::Index::new(accessors.len() as u32);
    accessors.push(gltf_json::Accessor {
        buffer_view: Some(vertex_buffer_view_idx),
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
    });
    let (min, max) = bounding_coords3d(normals);
    let normals_accessor_idx: Index<Accessor> = gltf_json::Index::new(accessors.len() as u32);
    accessors.push(gltf_json::Accessor {
        buffer_view: Some(normals_buffer_view_idx),
        byte_offset: 0,
        count: normals.len() as u32,
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
    });
    let indices_accessor_idx = gltf_json::Index::new(accessors.len() as u32);
    accessors.push(gltf_json::Accessor {
        buffer_view: Some(indices_buffer_view_idx),
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
    });

    //
    // Primitive
    //

    let mut attributes = std::collections::HashMap::new();
    attributes.insert(
        Valid(gltf_json::mesh::Semantic::Positions),
        positions_accessor_idx,
    );
    //
    // Material related objects
    //
    let mut images = Vec::new();
    let mut textures = Vec::new();
    let mut materials = Vec::new();
    let mut samplers = Vec::new();
    let material = if let Some((png_data, uv)) = material {
        let (png_ofs, png_len) = append_bytes(&mut bin, png_data);
        let (uv_ofs, uv_len) = append_bytes(&mut bin, uv.flatten());
        let png_buffer_view_idx = gltf_json::Index::new(buffer_views.len() as u32);
        buffer_views.push(gltf_json::buffer::View {
            buffer: gltf_json::Index::new(0),
            byte_length: png_len,
            byte_offset: Some(png_ofs),
            byte_stride: None,
            extensions: Default::default(),
            extras: Default::default(),
            name: None,
            target: Some(Valid(gltf_json::buffer::Target::ArrayBuffer)),
        });
        let uv_buffer_view_idx = gltf_json::Index::new(buffer_views.len() as u32);
        buffer_views.push(gltf_json::buffer::View {
            buffer: gltf_json::Index::new(0),
            byte_length: uv_len,
            byte_offset: Some(uv_ofs),
            byte_stride: None,
            extensions: Default::default(),
            extras: Default::default(),
            name: None,
            target: Some(Valid(gltf_json::buffer::Target::ArrayBuffer)),
        });
        let (min, max) = bounding_coords2d(uv);
        let uv_accessor_idx = gltf_json::Index::new(accessors.len() as u32);
        accessors.push(gltf_json::Accessor {
            buffer_view: Some(uv_buffer_view_idx),
            byte_offset: 0,
            count: vertices.len() as u32,
            component_type: Valid(gltf_json::accessor::GenericComponentType(
                gltf_json::accessor::ComponentType::F32,
            )),
            extensions: Default::default(),
            extras: Default::default(),
            type_: Valid(gltf_json::accessor::Type::Vec2),
            min: Some(gltf_json::Value::from(Vec::from(min))),
            max: Some(gltf_json::Value::from(Vec::from(max))),
            name: None,
            normalized: false,
            sparse: None,
        });
        attributes.insert(
            Valid(gltf_json::mesh::Semantic::TexCoords(0)),
            uv_accessor_idx,
        );

        let image_idx = gltf_json::Index::new(images.len() as u32);
        images.push(gltf_json::image::Image {
            name: None,
            buffer_view: Some(png_buffer_view_idx),
            mime_type: Some(MimeType("image/png".to_string())),
            uri: None,
            extensions: None,
            extras: Default::default(),
        });
        let sampler_idx = gltf_json::Index::new(samplers.len() as u32);
        samplers.push(gltf_json::texture::Sampler {
            mag_filter: Some(Valid(gltf::texture::MagFilter::Linear)),
            min_filter: Some(Valid(gltf::texture::MinFilter::Linear)),
            wrap_s: Valid(gltf::texture::WrappingMode::Repeat),
            wrap_t: Valid(gltf::texture::WrappingMode::Repeat),
            name: None,
            extensions: None,
            extras: Default::default(),
        });

        let info = gltf_json::extensions::texture::Info {
            texture_transform: Some(TextureTransform {
                offset: TextureTransformOffset([0.0, 0.0]),
                rotation: TextureTransformRotation(0.0),
                scale: TextureTransformScale([0.0, 0.0]),
                tex_coord: Some(uv_accessor_idx.value() as u32),
                extras: Default::default(),
            }),
        };
        let texture_idx = gltf_json::Index::new(textures.len() as u32);
        textures.push(gltf_json::texture::Texture {
            name: None,
            sampler: Some(sampler_idx),
            source: image_idx,
            extensions: Some(gltf_json::extensions::texture::Texture {}),
            extras: Some(RawValue::from_string(serde_json::to_string(&info)?)?),
        });

        let pbr_metallic_roughness = gltf_json::material::PbrMetallicRoughness {
            base_color_factor: PbrBaseColorFactor([1.0, 1.0, 1.0, 1.0]),
            metallic_factor: gltf_json::material::StrengthFactor(0.0),
            roughness_factor: gltf_json::material::StrengthFactor(0.9),
            base_color_texture: Some(gltf_json::texture::Info {
                index: texture_idx,
                tex_coord: 0,
                extensions: None,
                extras: Default::default(),
            }),
            ..Default::default()
        };
        let material_idx = Some(gltf_json::Index::new(materials.len() as u32));
        materials.push(gltf_json::material::Material {
            pbr_metallic_roughness,
            ..Default::default()
        });
        material_idx
    } else {
        None
    };
    let primitive = gltf_json::mesh::Primitive {
        attributes: {
            let mut map = std::collections::HashMap::new();
            map.insert(
                Valid(gltf_json::mesh::Semantic::Positions),
                positions_accessor_idx,
            );
            map.insert(
                Valid(gltf_json::mesh::Semantic::Normals),
                normals_accessor_idx,
            );
            map
        },
        extensions: Default::default(),
        extras: Default::default(),
        indices: Some(indices_accessor_idx),
        material,
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
    let bin_size = bin.len() as u32;
    let buffer = gltf_json::Buffer {
        byte_length: bin_size,
        extensions: Default::default(),
        extras: Default::default(),
        name: None,
        uri: None,
    };
    let root = gltf_json::Root {
        accessors,
        buffers: vec![buffer],
        buffer_views,
        meshes: vec![mesh],
        nodes: vec![node],
        scenes: vec![gltf_json::Scene {
            extensions: Default::default(),
            extras: Default::default(),
            name: None,
            nodes: vec![gltf_json::Index::new(0)],
        }],
        images,
        textures,
        materials,
        samplers,
        extensions_used: vec!["KHR_texture_transform".to_string()],
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
fn main() -> Result<()> {
    let args: Args = argh::from_env();
    if let Some(path) = args.input {
        run_input(&path)
    } else {
        Err(anyhow!("Run vacation --help for more information."))
    }
}
