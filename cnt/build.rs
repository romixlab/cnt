use std::{env, fs, path::PathBuf};

fn main() {
    println!("cargo:rerun-if-env-changed=CNT_RAM_BUFFER_SIZE_WORDS");
    println!("cargo:rerun-if-env-changed=CNT_BKP_BUFFER_SIZE_WORDS");

    let ram_size = env::var("CNT_RAM_BUFFER_SIZE_WORDS")
        .map(|s| {
            s.parse()
                .expect("could not parse CNT_RAM_BUFFER_SIZE_WORDS as usize")
        })
        .unwrap_or(64_usize);
    let bkp_size = env::var("CNT_BKP_BUFFER_SIZE_WORDS")
        .map(|s| {
            s.parse()
                .expect("could not parse CNT_BKP_BUFFER_SIZE_WORDS as usize")
        })
        .unwrap_or(0_usize);

    let out_dir_path = PathBuf::from(env::var_os("OUT_DIR").unwrap());
    let out_file_path = out_dir_path.join("consts.rs");

    fs::write(
        out_file_path,
        format!(
            "/// RAM counters buffer size (default: 64 words = 256 bytes).
            ///
            /// Can be customized by setting the `CNT_RAM_BUFFER_SIZE_WORDS` environment variable.
            /// Use a power of 2 for best performance.
            pub(crate) const RAM_BUF_SIZE: usize = {};

            /// BKP counters buffer size (default: 0 words).
            ///
            /// Can be customized by setting the `CNT_BKP_BUFFER_SIZE_WORDS` environment variable.
            /// Use a power of 2 for best performance.
            pub(crate) const BKP_BUF_SIZE: usize = {};",
            ram_size, bkp_size
        ),
    )
    .unwrap();

    // Put the linker script where linker can find it.
    let out = &PathBuf::from(env::var_os("OUT_DIR").unwrap());
    fs::write(out.join("cnt.x"), include_bytes!("cnt.x")).unwrap();
    println!("cargo:rustc-link-search={}", out.display());
    println!("cargo:rerun-if-changed=cnt.x");
}
