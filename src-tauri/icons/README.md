# Icons

Placeholder. Перед первой `cargo tauri build` нужно положить сюда:

- `32x32.png`
- `128x128.png`
- `128x128@2x.png`
- `icon.icns` (macOS, можно опустить — сборка только под Windows)
- `icon.ico` (Windows)

Сгенерировать можно через:

```bash
cargo tauri icon path/to/source-1024x1024.png
```

Это положит все нужные форматы в `src-tauri/icons/`. `cargo tauri dev`
работает и без них — иконки требуются только для bundle/.msi сборки.
