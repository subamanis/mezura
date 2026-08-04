fn main() {
    println!("cargo:rerun-if-changed=data");
    println!("cargo:rerun-if-changed=Changelog");
}
