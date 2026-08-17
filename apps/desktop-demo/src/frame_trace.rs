//! `UiFrame` 的稳定调试投影。
//!
//! 这是 demo 宿主的只读观测格式，不是另一个场景描述或 renderer 输入。
//! 它直接从已经 resolve 完成的 `tela_contract::UiFrame` 生成，因此任一 SDK renderer
//! 都可观测完全相同的逻辑帧。

use std::fmt::Write;

use tela_contract::{BorderRadius, BorderStroke, ClipRect, Color, DrawPayload, Rect, UiFrame};

pub(crate) fn to_json(frame: &UiFrame) -> String {
    let mut output = String::new();
    write!(
        output,
        r#"{{"viewport":{{"width":{},"height":{}}},"commands":["#,
        frame.viewport.width, frame.viewport.height
    )
    .expect("写入 String 不会失败");
    for (index, command) in frame.commands.iter().enumerate() {
        if index != 0 {
            output.push(',');
        }
        output.push_str(r#"{"geometry":"#);
        write_rect(&mut output, command.geometry);
        output.push_str(r#","clip":"#);
        write_clip(&mut output, command.clip);
        output.push_str(r#","payload":"#);
        write_payload(&mut output, &command.payload);
        output.push('}');
    }
    output.push_str(r#"],"hit_regions":["#);
    for (index, region) in frame.hit_regions.iter().enumerate() {
        if index != 0 {
            output.push(',');
        }
        output.push_str(r#"{"node_id":"#);
        write_json_string(&mut output, &format!("{:?}", region.node_id));
        output.push_str(r#","rect":"#);
        write_rect(&mut output, region.rect);
        output.push_str(r#","clip":"#);
        write_clip(&mut output, region.clip);
        output.push('}');
    }
    output.push_str(r#"],"scroll_bounds":["#);
    for (index, bounds) in frame.scroll_bounds.iter().enumerate() {
        if index != 0 {
            output.push(',');
        }
        output.push_str(r#"{"node_id":"#);
        write_json_string(&mut output, &format!("{:?}", bounds.node_id));
        output.push_str(r#","key":"#);
        write_json_string(&mut output, &bounds.key.0);
        output.push_str(r#","viewport":"#);
        write_rect(&mut output, bounds.viewport);
        write!(
            output,
            r#","content_width":{},"content_height":{},"max_offset_x":{},"max_offset_y":{}"#,
            bounds.content_width, bounds.content_height, bounds.max_offset_x, bounds.max_offset_y
        )
        .expect("写入 String 不会失败");
        output.push('}');
    }
    output.push_str("]}");
    output
}

fn write_rect(output: &mut String, rect: Rect) {
    write!(
        output,
        r#"{{"x":{},"y":{},"w":{},"h":{}}}"#,
        rect.x, rect.y, rect.w, rect.h
    )
    .expect("写入 String 不会失败");
}

fn write_clip(output: &mut String, clip: Option<ClipRect>) {
    match clip {
        Some(clip) => {
            output.push_str(r#"{"rect":"#);
            write_rect(output, clip.rect);
            output.push('}');
        }
        None => output.push_str("null"),
    }
}

fn write_payload(output: &mut String, payload: &DrawPayload) {
    match payload {
        DrawPayload::Rect { fill, border } => {
            output.push_str(r#"{"kind":"rect","fill":"#);
            write_color_option(output, *fill);
            output.push_str(r#","border":"#);
            write_border_option(output, *border);
            output.push('}');
        }
        DrawPayload::RoundedRect {
            fill,
            border,
            radius,
        } => {
            output.push_str(r#"{"kind":"rounded_rect","fill":"#);
            write_color_option(output, *fill);
            output.push_str(r#","border":"#);
            write_border_option(output, *border);
            output.push_str(r#","radius":"#);
            write_radius(output, *radius);
            output.push('}');
        }
        DrawPayload::Image { texture } => {
            output.push_str(r#"{"kind":"image","texture":"#);
            write_json_string(output, &texture.0);
            output.push('}');
        }
        DrawPayload::Text { text, baseline_y } => {
            output.push_str(r#"{"kind":"text","text":"#);
            write_json_string(output, &text.text);
            output.push_str(r#","font":"#);
            write_json_string(output, text.font.as_str());
            write!(
                output,
                r#","font_size":{},"line_height":{},"baseline_y":{},"color":"#,
                text.font_size, text.line_height, baseline_y
            )
            .expect("写入 String 不会失败");
            write_color(output, text.color);
            output.push('}');
        }
        other => {
            // 保留完整 Debug 文本而不是静默丢弃其他核心命令，以便场景扩展时立即暴露。
            output.push_str(r#"{"kind":"unprojected","debug":"#);
            write_json_string(output, &format!("{other:?}"));
            output.push('}');
        }
    }
}

fn write_color_option(output: &mut String, color: Option<Color>) {
    match color {
        Some(color) => write_color(output, color),
        None => output.push_str("null"),
    }
}

fn write_color(output: &mut String, color: Color) {
    write!(
        output,
        r#"{{"r":{},"g":{},"b":{},"a":{}}}"#,
        color.r, color.g, color.b, color.a
    )
    .expect("写入 String 不会失败");
}

fn write_border_option(output: &mut String, border: Option<BorderStroke>) {
    match border {
        Some(border) => {
            output.push_str(r#"{"color":"#);
            write_color(output, border.color);
            write!(output, r#","width":{}}}"#, border.width).expect("写入 String 不会失败");
        }
        None => output.push_str("null"),
    }
}

fn write_radius(output: &mut String, radius: BorderRadius) {
    write!(
        output,
        r#"{{"top_left":{},"top_right":{},"bottom_right":{},"bottom_left":{}}}"#,
        radius.top_left, radius.top_right, radius.bottom_right, radius.bottom_left
    )
    .expect("写入 String 不会失败");
}

fn write_json_string(output: &mut String, value: &str) {
    output.push('"');
    for character in value.chars() {
        match character {
            '"' => output.push_str("\\\""),
            '\\' => output.push_str("\\\\"),
            '\n' => output.push_str("\\n"),
            '\r' => output.push_str("\\r"),
            '\t' => output.push_str("\\t"),
            character if character.is_control() => {
                write!(output, "\\u{:04x}", character as u32).expect("写入 String 不会失败");
            }
            character => output.push(character),
        }
    }
    output.push('"');
}

#[cfg(test)]
mod tests {
    use tela_contract::{Color, DrawCommand, DrawPayload, UiFrame, Viewport};

    use super::to_json;

    #[test]
    fn writes_rect_payload_as_structured_json() {
        let frame = UiFrame {
            viewport: Viewport {
                width: 1.0,
                height: 1.0,
            },
            commands: vec![DrawCommand {
                geometry: tela_contract::Rect {
                    x: 0.0,
                    y: 0.0,
                    w: 1.0,
                    h: 1.0,
                },
                clip: None,
                payload: DrawPayload::Rect {
                    fill: Some(Color::BLUE),
                    border: None,
                },
            }],
            hit_regions: Vec::new(),
            scroll_bounds: Vec::new(),
        };

        assert_eq!(
            to_json(&frame),
            "{\"viewport\":{\"width\":1,\"height\":1},\"commands\":[{\"geometry\":{\"x\":0,\"y\":0,\"w\":1,\"h\":1},\"clip\":null,\"payload\":{\"kind\":\"rect\",\"fill\":{\"r\":0,\"g\":0,\"b\":1,\"a\":1},\"border\":null}}],\"hit_regions\":[],\"scroll_bounds\":[]}"
        );
    }

    #[test]
    fn projects_scroll_bounds_as_observable_frame_metadata() {
        let frame = UiFrame {
            viewport: Viewport {
                width: 1.0,
                height: 1.0,
            },
            commands: Vec::new(),
            hit_regions: Vec::new(),
            scroll_bounds: vec![tela_contract::ScrollBounds {
                node_id: tela_contract::NodeId(7),
                key: tela_contract::SemanticKey("detail".to_owned()),
                viewport: tela_contract::Rect {
                    x: 2.0,
                    y: 3.0,
                    w: 4.0,
                    h: 5.0,
                },
                content_width: 8.0,
                content_height: 9.0,
                max_offset_x: 4.0,
                max_offset_y: 4.0,
            }],
        };

        assert_eq!(
            to_json(&frame),
            "{\"viewport\":{\"width\":1,\"height\":1},\"commands\":[],\"hit_regions\":[],\"scroll_bounds\":[{\"node_id\":\"NodeId(7)\",\"key\":\"detail\",\"viewport\":{\"x\":2,\"y\":3,\"w\":4,\"h\":5},\"content_width\":8,\"content_height\":9,\"max_offset_x\":4,\"max_offset_y\":4}]}"
        );
    }
}
