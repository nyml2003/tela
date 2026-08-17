# Material Symbols Rounded 子集来源

- 上游：Google Fonts `material-symbols` 仓库的
  `fonts/variablefont/MaterialSymbolsRounded[FILL,GRAD,opsz,wght].ttf`。
- 许可证：Apache-2.0，完整文本见同目录 `LICENSE-MaterialSymbols.txt`。
- 本仓库文件：`MaterialSymbolsRounded-subset.ttf`，只含
  `tela_contract::IconName` 当前映射到的字形。
- 固定实例轴：`FILL=0`、`wght=400`、`GRAD=0`、`opsz=24`。

在 `nix develop` 环境中可用 `fonttools` 重新生成子集；更新图标映射时必须同时更新码位列表与
子集文件：

```bash
fonttools varLib.instancer MaterialSymbolsRounded[FILL,GRAD,opsz,wght].ttf \
  FILL=0 wght=400 GRAD=0 opsz=24 \
  -o /tmp/MaterialSymbolsRounded-static.ttf
pyftsubset /tmp/MaterialSymbolsRounded-static.ttf \
  --output-file=assets/fonts/MaterialSymbolsRounded-subset.ttf \
  --unicodes=U+E145,U+E92E,U+F097,U+E14D,U+E9A1,U+E938,U+F09A,U+E893,U+E166,U+EF7A,U+E2C7,U+E2C8,U+E873,U+E3F4,U+EB2C,U+E9B2,U+E8EF,U+E9B0,U+E164,U+E152,U+E5CC,U+E5D2,U+E5D3 \
  --no-hinting
```

上游文件下载与本地子集化均属于构建期维护操作；运行时只读取仓库内的受控 TTF 字节。
