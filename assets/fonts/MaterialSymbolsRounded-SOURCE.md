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
  --unicodes=U+E145,U+E92E,U+F097,U+E14D,U+E9A1,U+E938,U+F09A,U+E893,U+E166,U+EF7A,U+E2C7,U+E2C8,U+E873,U+E3F4,U+EB2C,U+E9B2,U+E92E,U+E8EF,U+E9B0,U+E164,U+E152,U+E5CC,U+E5C4,U+E5D2,U+E5D3,U+E5CD,U+E931,U+E3C6,U+E3E0,U+E15A,U+F08B,U+E14F,U+E161,U+EB60,U+E162,U+E881,U+E238,U+E23F,U+E249,U+E236,U+E234,U+E237,U+E245,U+E8CE,U+E15B,U+F08F,U+E92B,U+E173,U+EF87,U+E674,U+E415,U+E2CC,U+E226,U+E250,U+E16F,U+F090,U+F09B,U+F15C,U+E2C0,U+E2C3,U+E9A1,U+EB2C,U+E169,U+E8AD,U+E5C8,U+E5D8,U+E5DB,U+E5CB,U+E5CE,U+E5CF,U+E5D0,U+E5D1,U+E89E,U+E89E,U+E9B2,U+E9BD,U+E668,U+F0BE,U+E888,U+F8B6,U+F083,U+E88E,U+E8FD,U+EF76,U+E899,U+E898,U+E8F4,U+E8F5,U+E5D5,U+E627,U+E8B3,U+E8EF,U+E8F0,U+E8F1,U+E9B0,U+EF4F,U+EB32,U+E429,U+E265,U+E8FF,U+E900,U+F0D3,U+EA21,U+EA21,U+F20B,U+E159,U+E0C9,U+E24C,U+E80D,U+E7F5,U+E037,U+E034,U+E047,U+E044,U+E045,U+E050,U+E04F,U+E31D,U+E684,U+E412 \
  --no-hinting
```

上游文件下载与本地子集化均属于构建期维护操作；运行时只读取仓库内的受控 TTF 字节。
