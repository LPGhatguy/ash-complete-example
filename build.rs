use std::env;
use std::process::Command;

static SHADERS: &'static [&'static str] = &[
    "triangle.frag",
    "triangle.vert",
];

fn main() {
    let out_dir = env::var("OUT_DIR").unwrap();

    for file_name in SHADERS {
        let in_file = format!("shaders/{}", file_name);
        let out_file = format!("{}/{}.spv", out_dir, file_name.replace(".", "-"));

        Command::new("glslc")
            .args(&["-o", &out_file])
            .arg(in_file)
            .status().unwrap();
    }
}