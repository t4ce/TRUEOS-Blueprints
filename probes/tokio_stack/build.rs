fn main() {
    let protoc = protoc_bin_vendored::protoc_bin_path()
        .expect("tokio_stack: vendored protoc must be available");
    // SAFETY: Cargo runs this build script as a single-threaded process before
    // tonic-prost-build reads PROTOC.
    unsafe {
        std::env::set_var("PROTOC", protoc);
    }

    tonic_prost_build::configure()
        .build_client(true)
        .build_server(true)
        .compile_protos(&["proto/stack.proto"], &["proto"])
        .expect("tokio_stack: compile gRPC witness protocol");

    println!("cargo:rerun-if-changed=proto/stack.proto");
}
