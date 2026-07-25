//! Encodes the embedded web fonts into the folio's `@font-face` block.
//!
//! The faces are compile-time constants, so the base64 they travel as is one
//! too: doing it here keeps megabytes of encoding out of every render, and out
//! of the binary's startup. `just fonts` vendors the woff2 files this reads.

use std::{env, fs, path::Path};

use base64::{Engine, engine::general_purpose::STANDARD};

/// One embedded face: the family and posture the stylesheet asks for, the
/// weight range the variable file covers, and the vendored woff2 file.
///
/// Junicode 2 (serif body, roman + italic) and Fira Code (monospace) are
/// variable, so one file per posture spans every weight; UnifrakturCook is the
/// single-weight blackletter used for headings and dropped versals.
const FACES: [Face; 4] = [
    Face {
        family: "Junicode",
        style: "normal",
        weight: "300 700",
        file: "JunicodeVF-Roman.woff2",
    },
    Face {
        family: "Junicode",
        style: "italic",
        weight: "300 700",
        file: "JunicodeVF-Italic.woff2",
    },
    Face {
        family: "UnifrakturCook",
        style: "normal",
        weight: "400",
        file: "UnifrakturCook.woff2",
    },
    Face {
        family: "Fira Code",
        style: "normal",
        weight: "300 700",
        file: "FiraCode-VF.woff2",
    },
];

struct Face {
    family: &'static str,
    style: &'static str,
    weight: &'static str,
    file: &'static str,
}

fn main() {
    let fonts = Path::new("src/fonts");
    let mut block = String::new();

    for face in FACES {
        let path = fonts.join(face.file);
        println!("cargo::rerun-if-changed={}", path.display());
        let woff2 = fs::read(&path).unwrap_or_else(|error| {
            panic!("reading {}: {error}", path.display());
        });
        block.push_str(&format!(
            "@font-face{{font-family:\"{}\";font-style:{};font-weight:{};font-display:swap;\
             src:url(data:font/woff2;base64,{}) format(\"woff2\")}}",
            face.family,
            face.style,
            face.weight,
            STANDARD.encode(woff2),
        ));
    }

    let out = Path::new(&env::var("OUT_DIR").expect("cargo sets OUT_DIR")).join("font-faces.css");
    fs::write(&out, block).unwrap_or_else(|error| panic!("writing {}: {error}", out.display()));
}
