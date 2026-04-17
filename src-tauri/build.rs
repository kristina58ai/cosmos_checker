fn main() {
    tauri_build::build();

    // В Этапе 5 сюда добавится tonic_build::configure() для gRPC-кодогена
    // из src-tauri/proto/*.proto.
}
