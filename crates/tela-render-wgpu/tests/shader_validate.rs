//! shader.wgsl 编译期校验。
//!
//! T1 语法+类型：naga 解析 + Validator 校验——WGSL 错误不再运行时才炸；
//! T2 入口完整性：当前最小 renderer 只允许 Solid/Rounded 两组入口。

use naga::front::wgsl::parse_str;

const SHADER: &str = include_str!("../src/shader.wgsl");

fn parse() -> naga::Module {
    parse_str(SHADER).expect("shader.wgsl 必须可解析（WGSL 语法错误）")
}

/// T1：语法 + 类型校验。
#[test]
fn wgsl_parses_and_validates() {
    let module = parse();
    naga::valid::Validator::new(
        naga::valid::ValidationFlags::all(),
        naga::valid::Capabilities::all(),
    )
    .validate(&module)
    .expect("shader.wgsl 必须通过 naga 类型校验");
}

/// T2：四个必需入口存在，其他能力没有 shader 入口。
#[test]
fn required_entry_points_exist() {
    let module = parse();
    let names: Vec<&str> = module
        .entry_points
        .iter()
        .map(|e| e.name.as_str())
        .collect();
    for required in ["vs_solid", "fs_solid", "vs_rounded", "fs_rounded"] {
        assert!(
            names.contains(&required),
            "缺少 entry point: {required}，现有: {names:?}"
        );
    }
    assert_eq!(names.len(), 4, "最小 shader 不应偷偷带入其他能力入口");
}
