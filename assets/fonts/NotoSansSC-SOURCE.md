# Noto Sans SC 子集来源

- 上游：notofonts/noto-cjk `Sans/Variable/OTC/NotoSansCJK-VF.otf.ttc` 的 SC face。
- 版本：2.004（本机 nixpkgs `noto-fonts-cjk-sans-2.004`）。
- 许可证：SIL Open Font License 1.1，完整文本见 `LICENSE-NotoSansCJK.txt`。
- 本仓库产物：Regular 400 与 Medium 500 两个静态 OTF 子集。
- 字符范围：当前仓库 Rust、Markdown、TypeScript 与 HTML 源文件出现的字符；更新产品文案后应重跑。

维护时先从 SC 可变字体生成字符子集，再固定字重并将 CFF2 降级为 ab_glyph 可解析的 CFF：

```bash
SOURCE=$(fc-match 'Noto Sans CJK SC' -f '%{file}\n' | head -1)
rg --no-filename '.' apps crates docs products \
  --glob '*.rs' --glob '*.md' --glob '*.ts' --glob '*.html' \
  | pyftsubset "$SOURCE" --font-number=2 --text-file=/dev/stdin \
      --output-file=/tmp/NotoSansSC-subset.otf --layout-features='*' \
      --name-IDs='*' --name-languages='*' --name-legacy --glyph-names \
      --symbol-cmap --legacy-cmap --notdef-glyph --notdef-outline \
      --recommended-glyphs --drop-tables+=DSIG
fonttools varLib.instancer /tmp/NotoSansSC-subset.otf wght=400 \
  --static --downgrade-cff2 --update-name-table \
  -o assets/fonts/NotoSansSC-Regular-subset.otf
fonttools varLib.instancer /tmp/NotoSansSC-subset.otf wght=500 \
  --static --downgrade-cff2 --update-name-table \
  -o assets/fonts/NotoSansSC-Medium-subset.otf
```

运行时只读取仓库内受控字节，不扫描或加载系统字体。
