fn main() {
    println!("cargo:rerun-if-changed=haru.rc");
    println!("cargo:rerun-if-changed=../../packaging/haru.ico");

    // Only does anything when building for Windows. Required rather than
    // optional, so a build machine without a resource compiler fails instead
    // of quietly shipping an exe with no icon.
    if let Err(error) = embed_resource::compile("haru.rc", embed_resource::NONE).manifest_required()
    {
        println!("cargo:warning=could not embed haru's icon: {error}");
        std::process::exit(1);
    }
}
