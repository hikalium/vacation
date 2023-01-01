use std::{fs, io};

use anyhow::anyhow;
use anyhow::Result;
use argh::FromArgs;
use gltf::Node;
use gltf::Semantic;

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

fn run(path: &str) -> Result<()> {
    let file = fs::File::open(&path)?;
    let reader = io::BufReader::new(file);
    let gltf = gltf::Gltf::from_reader(reader)?;
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
                p.bounding_box()
            );
            if let Some(a) = p.get(&Semantic::Positions) {
                println!("Positions: {:?} {:?}", a.dimensions(), a.data_type());
            }
        }
    }
    Ok(())
}

fn main() -> Result<()> {
    let args: Args = argh::from_env();
    if let Some(path) = args.input {
        run(&path)
    } else {
        Err(anyhow!("Run vacation --help for more information."))
    }
}
