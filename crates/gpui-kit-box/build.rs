fn main() {
    println!("cargo:rerun-if-changed=looui.rc");
    println!("cargo:rerun-if-changed=assets/app-icon.ico");

    if cfg!(windows) {
        embed_resource::compile("looui.rc", embed_resource::NONE)
            .manifest_optional()
            .expect("嵌入 looui.rc 失败");
    }
}
