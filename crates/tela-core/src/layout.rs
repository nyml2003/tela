//! 布局引擎（见 006-布局引擎）。
//!
//! - `LayoutEngine` trait：纯函数测量（同节点同约束同盒子）；
//! - `DefaultLayoutEngine`：Flex（含 wrap 开关）/ Stack / ScrollView / 逻辑容器；
//! - 尺寸三层解析：基准层 → 本地 MinMax 钳制 → 父 Constraints 二次钳制（区间交集，空区间返回
//!   `UiLayoutError::MinConstraintViolation`）；
//! - 单节点测量缓存（路线 A，见 004-更新策略与状态保持 7.1）：只缓存原语叶子测量，
//!   纯输入失效判定（节点指针 + 约束），缓存只是查表加速，不影响纯函数结果。

use std::collections::HashMap;
use tela_contract::{
    BaseSize, Constraints, ContentConcern, CrossAlign, FlexDirection, Insets, LayoutBox, MainAlign,
    MinMax, NodeKind, Size, StackAlign, StackLayer, TextMeasureRequest, TextMeasurer,
    UiLayoutError, UiNode,
};

/// 布局引擎抽象（见 006-布局引擎 2）。
///
/// `measure` 是纯函数：相同节点 + 相同约束 → 相同盒子；不读取 `visual`/`interact`/`identity`。
pub trait LayoutEngine {
    /// 测量节点及其子树，输出以节点原点为原点的盒子树。
    fn measure(
        &mut self,
        node: &UiNode,
        constraints: Constraints,
    ) -> Result<LayoutBox, UiLayoutError>;
}

/// 默认布局引擎：Flex（wrap 开关）/ Stack / ScrollView / 逻辑容器（见 010-落地路线 M2）。
pub struct DefaultLayoutEngine<'a, M: TextMeasurer + ?Sized> {
    text_measurer: &'a M,
    cache: MeasureCache,
}

impl<'a, M: TextMeasurer + ?Sized> DefaultLayoutEngine<'a, M> {
    /// 以文本度量器构造引擎。
    pub fn new(text_measurer: &'a M) -> Self {
        Self {
            text_measurer,
            cache: MeasureCache::default(),
        }
    }

    /// 清空测量缓存（结果不变，缓存只是加速，见 004-7.1）。
    pub fn clear_cache(&mut self) {
        self.cache.map.clear();
        self.cache.hits = 0;
        self.cache.misses = 0;
    }

    /// 测量缓存命中/未命中计数（测试用）。
    pub fn cache_stats(&self) -> (usize, usize) {
        (self.cache.hits, self.cache.misses)
    }
}

impl<M: TextMeasurer + ?Sized> LayoutEngine for DefaultLayoutEngine<'_, M> {
    fn measure(
        &mut self,
        node: &UiNode,
        constraints: Constraints,
    ) -> Result<LayoutBox, UiLayoutError> {
        self.measure_inner(node, constraints)
    }
}

impl<'a, M: TextMeasurer + ?Sized> DefaultLayoutEngine<'a, M> {
    /// 节点分发：逻辑容器透明透传 / 原语叶子测量（缓存）/ 布局容器各自算法（递归测 children）。
    fn measure_inner(
        &mut self,
        node: &UiNode,
        constraints: Constraints,
    ) -> Result<LayoutBox, UiLayoutError> {
        if node.kind.is_logical_container() {
            let children = self.measure_children(node, constraints)?;
            self.measure_logical(node, children)
        } else if node.kind.is_primitive() {
            self.measure_leaf(node, constraints)
        } else {
            let inner = children_constraints(node, constraints);
            let children = self.measure_children(node, inner)?;
            self.measure_node(node, constraints, children)
        }
    }

    /// 单节点测量：children 已按 `children_constraints` 测好，组装节点自身盒（Dirty 逐节点缓存用）。
    pub(crate) fn measure_node(
        &mut self,
        node: &UiNode,
        constraints: Constraints,
        children: Vec<LayoutBox>,
    ) -> Result<LayoutBox, UiLayoutError> {
        if node.kind.is_logical_container() {
            self.measure_logical(node, children)
        } else if node.kind.is_primitive() {
            self.measure_leaf(node, constraints)
        } else {
            match &node.kind {
                NodeKind::Flex => self.measure_flex(node, constraints, children),
                NodeKind::Stack => self.measure_stack(node, constraints, children),
                NodeKind::ScrollView => self.measure_scroll_view(node, constraints, children),
                NodeKind::VirtualListView(spec) => {
                    self.measure_virtual_list(node, *spec, constraints, children)
                }
                _ => unreachable!("布局容器均已覆盖"),
            }
        }
    }

    // ---------- 逻辑容器：零几何、透明，盒 = 子节点包围盒 ----------

    fn measure_logical(
        &mut self,
        _node: &UiNode,
        children: Vec<LayoutBox>,
    ) -> Result<LayoutBox, UiLayoutError> {
        let w = children.iter().map(|c| c.x + c.w).fold(0.0, f32::max);
        let h = children.iter().map(|c| c.y + c.h).fold(0.0, f32::max);
        let first_baseline = propagated_baseline(&children);
        Ok(LayoutBox {
            x: 0.0,
            y: 0.0,
            w,
            h,
            first_baseline,
            children,
        })
    }

    // ---------- 原语叶子：单节点测量（路线 A 缓存） ----------

    fn measure_leaf(
        &mut self,
        node: &UiNode,
        constraints: Constraints,
    ) -> Result<LayoutBox, UiLayoutError> {
        let key = (
            node as *const UiNode as usize,
            constraints.min_w.to_bits(),
            constraints.max_w.to_bits(),
            constraints.min_h.to_bits(),
            constraints.max_h.to_bits(),
        );
        if let Some((w, h, first_baseline)) = self.cache.map.get(&key) {
            self.cache.hits += 1;
            return Ok(LayoutBox {
                x: 0.0,
                y: 0.0,
                w: *w,
                h: *h,
                first_baseline: *first_baseline,
                children: Vec::new(),
            });
        }
        self.cache.misses += 1;

        let text_metrics = match &node.content {
            Some(ContentConcern::Text(text)) => {
                Some(self.text_measurer.measure(&TextMeasureRequest {
                    text: &text.text,
                    font: &text.font,
                    font_size: text.font_size,
                    line_height: text.line_height,
                    // 文本 Auto 尺寸与换行依赖可用宽度（见 003-6）。
                    max_width: Some(constraints.max_w),
                }))
            }
            _ => None,
        };

        let w = self.resolve_size_axis(
            node,
            Axis::Width,
            AxisSize {
                percent_base: constraints.max_w,
                fill_base: constraints.max_w,
                auto_fallback: text_metrics.map_or(0.0, |metrics| metrics.width),
                min: constraints.min_w,
                max: constraints.max_w,
            },
        )?;
        let h = self.resolve_size_axis(
            node,
            Axis::Height,
            AxisSize {
                percent_base: constraints.max_h,
                fill_base: constraints.max_h,
                auto_fallback: text_metrics.map_or(0.0, |metrics| metrics.height),
                min: constraints.min_h,
                max: constraints.max_h,
            },
        )?;
        let first_baseline = text_metrics.map(|metrics| metrics.first_baseline.clamp(0.0, h));
        self.cache.map.insert(key, (w, h, first_baseline));
        Ok(LayoutBox {
            x: 0.0,
            y: 0.0,
            w,
            h,
            first_baseline,
            children: Vec::new(),
        })
    }

    // ---------- Flex：统一容器 + wrap 开关（见 006-2.1） ----------

    fn measure_flex(
        &mut self,
        node: &UiNode,
        constraints: Constraints,
        measured: Vec<LayoutBox>,
    ) -> Result<LayoutBox, UiLayoutError> {
        let layout = node.layout.as_ref().cloned().unwrap_or_default();
        let (main_axis, cross_axis) = match layout.direction {
            FlexDirection::Row => (Axis::Width, Axis::Height),
            FlexDirection::Column => (Axis::Height, Axis::Width),
        };
        let inner = inner_constraints(constraints, &layout);

        // 子节点测量与内容尺寸（children 已按 inner 约束测好）。
        let (mut measured, mut final_main, content_main, content_cross) =
            self.flex_child_pipeline(node, measured, &inner, &layout, main_axis, cross_axis)?;

        // 容器自身尺寸：声明 or Auto → 内容尺寸（三层解析）。
        let self_main = self.resolve_self_axis(
            node,
            main_axis,
            content_main,
            axis_max(&constraints, main_axis),
            axis_min(&constraints, main_axis),
            axis_max(&constraints, main_axis),
        )?;
        let self_cross = self.resolve_self_axis(
            node,
            cross_axis,
            content_cross,
            axis_max(&constraints, cross_axis),
            axis_min(&constraints, cross_axis),
            axis_max(&constraints, cross_axis),
        )?;

        // 容器自身声明了非内容推导尺寸时，子节点约束必须基于最终内容区重测
        // （如声明宽 120 的容器不能按父宽 200 分配 Fill）。
        if declared_non_auto(node, main_axis) || declared_non_auto(node, cross_axis) {
            let inner_final = content_area_constraints(&layout, main_axis, self_main, self_cross);
            let re_measured = self.measure_children(node, inner_final)?;
            (measured, final_main, _, _) = self.flex_child_pipeline(
                node,
                re_measured,
                &inner_final,
                &layout,
                main_axis,
                cross_axis,
            )?;
        }

        let mut box_ = LayoutBox {
            x: 0.0,
            y: 0.0,
            w: 0.0,
            h: 0.0,
            first_baseline: None,
            children: Vec::new(),
        };
        set_axis_extent(&mut box_, main_axis, self_main);
        set_axis_extent(&mut box_, cross_axis, self_cross);

        // 盒内容区尺寸（摆放坐标系）。
        let area_main = content_area(&box_, &layout, main_axis);
        let area_cross = content_area(&box_, &layout, cross_axis);

        box_.children = if layout.wrap {
            self.place_flex_wrapped(
                node,
                measured,
                &final_main,
                &margins_of(node),
                &fill_flags(node, main_axis),
                main_axis,
                cross_axis,
                area_main,
                area_cross,
                &layout,
            )
        } else {
            self.place_flex_single_line(
                node,
                measured,
                &final_main,
                &margins_of(node),
                main_axis,
                cross_axis,
                area_main,
                area_cross,
                &layout,
            )
        };
        box_.first_baseline = propagated_baseline(&box_.children);
        Ok(box_)
    }

    /// 子节点测量流水线：fill 份额 → 最终主轴 → 内容尺寸。
    fn flex_child_pipeline(
        &mut self,
        node: &UiNode,
        measured: Vec<LayoutBox>,
        inner: &Constraints,
        layout: &tela_contract::LayoutConcern,
        main_axis: Axis,
        cross_axis: Axis,
    ) -> Result<(Vec<LayoutBox>, Vec<f32>, f32, f32), UiLayoutError> {
        let inner_main = axis_extent(inner, main_axis);
        let is_fill: Vec<bool> = node
            .children
            .iter()
            .map(|c| is_fill_axis(c, main_axis))
            .collect();
        let margins: Vec<Insets> = node.children.iter().map(margin_of).collect();

        // 主轴占用（fill 子不计）与 fill 份额。
        let used_main: f32 = node
            .children
            .iter()
            .zip(&measured)
            .zip(&is_fill)
            .zip(&margins)
            .map(|(((_c, b), fill), m)| {
                if *fill {
                    0.0
                } else {
                    axis_extent_of(b, main_axis) + margin_axis(m, main_axis)
                }
            })
            .sum::<f32>()
            + layout.gap * node.children.len().saturating_sub(1) as f32;
        let fill_count = is_fill.iter().filter(|f| **f).count();
        let fill_margin_total: f32 = margins
            .iter()
            .zip(&is_fill)
            .map(|(m, f)| if *f { margin_axis(m, main_axis) } else { 0.0 })
            .sum();
        let remaining_main = inner_main - used_main;
        let share = if fill_count > 0 {
            (remaining_main - fill_margin_total).max(0.0) / fill_count as f32
        } else {
            0.0
        };

        // 最终子盒主轴尺寸（fill 按份额三层解析）。
        let mut final_main: Vec<f32> = Vec::with_capacity(node.children.len());
        for (child, fill) in node.children.iter().zip(&is_fill) {
            if *fill {
                final_main.push(self.resolve_size_axis(
                    child,
                    main_axis,
                    AxisSize {
                        percent_base: inner_main,
                        fill_base: share,
                        auto_fallback: 0.0,
                        min: axis_min(inner, main_axis),
                        max: axis_max(inner, main_axis),
                    },
                )?);
            } else {
                let b = &measured[final_main.len()];
                final_main.push(axis_extent_of(b, main_axis));
            }
        }

        // 容器内容尺寸（主轴 Σ，交叉 max；Stretch 子由容器决定，不撑容器）。
        let content_main = if layout.wrap {
            self.wrap_content_main(
                node,
                &measured,
                &is_fill,
                &margins,
                &final_main,
                main_axis,
                inner_main,
                layout,
            )
        } else {
            final_main
                .iter()
                .zip(&margins)
                .map(|(m, mar)| m + margin_axis(mar, main_axis))
                .sum::<f32>()
                + layout.gap * node.children.len().saturating_sub(1) as f32
        };
        let content_cross = if baseline_alignment(layout, main_axis) {
            baseline_cross_extent(&measured, &margins, cross_axis)
        } else {
            measured
                .iter()
                .zip(&node.children)
                .zip(&margins)
                .map(|((b, child), m)| {
                    if layout.cross_align == CrossAlign::Stretch
                        && stretch_implicit(child, cross_axis)
                    {
                        0.0
                    } else {
                        axis_extent_of(b, cross_axis) + margin_axis(m, cross_axis)
                    }
                })
                .fold(0.0, f32::max)
        };
        Ok((measured, final_main, content_main, content_cross))
    }

    /// wrap=true 的容器内容主轴：多行 → 行容量（inner_main）；单行 → 行内容。
    #[allow(clippy::too_many_arguments)]
    fn wrap_content_main(
        &self,
        node: &UiNode,
        measured: &[LayoutBox],
        is_fill: &[bool],
        margins: &[Insets],
        final_main: &[f32],
        main_axis: Axis,
        inner_main: f32,
        layout: &tela_contract::LayoutConcern,
    ) -> f32 {
        let mut wrapped = false;
        let mut current_main = 0.0;
        for index in 0..node.children.len() {
            let w = if is_fill[index] {
                0.0
            } else {
                axis_extent_of(&measured[index], main_axis)
                    + margin_axis(&margins[index], main_axis)
            };
            if !wrapped && current_main > 0.0 && current_main + w > inner_main {
                wrapped = true;
            }
            current_main += w + layout.gap;
        }
        if wrapped {
            inner_main
        } else {
            final_main
                .iter()
                .zip(margins)
                .map(|(m, mar)| m + margin_axis(mar, main_axis))
                .sum::<f32>()
                + layout.gap * node.children.len().saturating_sub(1) as f32
        }
    }

    /// wrap=false：单行，全局 Fill 分配剩余空间。
    #[allow(clippy::too_many_arguments)]
    fn place_flex_single_line(
        &self,
        node: &UiNode,
        mut boxes: Vec<LayoutBox>,
        final_main: &[f32],
        margins: &[Insets],
        main_axis: Axis,
        cross_axis: Axis,
        area_main: f32,
        area_cross: f32,
        layout: &tela_contract::LayoutConcern,
    ) -> Vec<LayoutBox> {
        let origin_main = content_origin(layout, main_axis);
        let origin_cross = content_origin(layout, cross_axis);
        let total_main: f32 = final_main
            .iter()
            .zip(margins)
            .map(|(m, mar)| m + margin_axis(mar, main_axis))
            .sum::<f32>()
            + layout.gap * node.children.len().saturating_sub(1) as f32;
        let free = (area_main - total_main).max(0.0);
        let (leading, per_gap) = main_align_spacing(layout.main_align, free, node.children.len());

        let mut result = Vec::with_capacity(boxes.len());
        let mut cursor = origin_main + leading;
        let baseline_target = baseline_alignment(layout, main_axis)
            .then(|| baseline_target(&boxes, margins, cross_axis));
        for (index, child) in node.children.iter().enumerate() {
            let mut child_box = std::mem::take(&mut boxes[index]);
            let margin = &margins[index];
            let child_cross = placed_cross(
                layout.cross_align,
                child,
                cross_axis,
                area_cross,
                &child_box,
            );
            let child_baseline = baseline_of(&child_box, cross_axis);
            set_axis_extent(&mut child_box, main_axis, final_main[index]);
            set_axis_extent(&mut child_box, cross_axis, child_cross);
            set_axis_pos(
                &mut child_box,
                main_axis,
                cursor + margin_axis_start(margin, main_axis),
            );
            set_axis_pos(
                &mut child_box,
                cross_axis,
                match baseline_target {
                    Some(target) => origin_cross + target - child_baseline,
                    None => {
                        origin_cross
                            + cross_align_pos(
                                layout.cross_align,
                                child_cross + margin_axis(margin, cross_axis),
                                area_cross,
                                margin_axis_start(margin, cross_axis),
                            )
                    }
                },
            );
            cursor += final_main[index] + margin_axis(margin, main_axis) + layout.gap + per_gap;
            result.push(child_box);
        }
        result
    }

    /// wrap=true：自动换行，Fill 仅单行内部（跨行不共享空白）。
    #[allow(clippy::too_many_arguments)]
    fn place_flex_wrapped(
        &self,
        node: &UiNode,
        mut boxes: Vec<LayoutBox>,
        final_main: &[f32],
        margins: &[Insets],
        is_fill: &[bool],
        main_axis: Axis,
        cross_axis: Axis,
        area_main: f32,
        area_cross: f32,
        layout: &tela_contract::LayoutConcern,
    ) -> Vec<LayoutBox> {
        // 分行：主轴累计超出盒内容区则换行（fill 子不占位）。
        let mut rows: Vec<Vec<usize>> = Vec::new();
        let mut current: Vec<usize> = Vec::new();
        let mut current_main = 0.0;
        for index in 0..boxes.len() {
            let w = if is_fill[index] {
                0.0
            } else {
                final_main[index] + margin_axis(&margins[index], main_axis)
            };
            if !current.is_empty() && current_main + w > area_main {
                rows.push(std::mem::take(&mut current));
                current_main = 0.0;
            }
            current.push(index);
            current_main += w + layout.gap;
        }
        if !current.is_empty() {
            rows.push(current);
        }

        let origin_main = content_origin(layout, main_axis);
        let origin_cross = content_origin(layout, cross_axis);
        let mut row_cursor = origin_cross;
        let mut result: Vec<LayoutBox> = Vec::with_capacity(boxes.len());
        for row in &rows {
            // 行内占用与 fill 份额（fill 仅吃本行剩余）。
            let row_used: f32 = row
                .iter()
                .map(|&i| {
                    if is_fill[i] {
                        0.0
                    } else {
                        final_main[i] + margin_axis(&margins[i], main_axis)
                    }
                })
                .sum::<f32>()
                + layout.gap * row.len().saturating_sub(1) as f32;
            let fill_in_row = row.iter().filter(|&&i| is_fill[i]).count();
            let row_fill_margin: f32 = row
                .iter()
                .filter(|&&i| is_fill[i])
                .map(|&i| margin_axis(&margins[i], main_axis))
                .sum();
            let row_share = if fill_in_row > 0 {
                (area_main - row_used - row_fill_margin).max(0.0) / fill_in_row as f32
            } else {
                0.0
            };
            let row_main: Vec<f32> = row
                .iter()
                .map(|&i| {
                    if is_fill[i] {
                        self.resolve_size_axis(
                            &node.children[i],
                            main_axis,
                            AxisSize {
                                percent_base: area_main,
                                fill_base: row_share,
                                auto_fallback: 0.0,
                                min: 0.0,
                                max: area_main,
                            },
                        )
                        .unwrap_or(0.0)
                    } else {
                        final_main[i]
                    }
                })
                .collect();
            let row_content: f32 = row_main
                .iter()
                .zip(row)
                .map(|(m, &i)| m + margin_axis(&margins[i], main_axis))
                .sum::<f32>()
                + layout.gap * row.len().saturating_sub(1) as f32;
            let row_free = (area_main - row_content).max(0.0);
            let (leading, per_gap) = main_align_spacing(layout.main_align, row_free, row.len());
            let row_boxes: Vec<LayoutBox> = row.iter().map(|&i| boxes[i].clone()).collect();
            let row_margins: Vec<Insets> = row.iter().map(|&i| margins[i]).collect();
            let row_height = if baseline_alignment(layout, main_axis) {
                baseline_cross_extent(&row_boxes, &row_margins, cross_axis)
            } else {
                row_boxes
                    .iter()
                    .zip(&row_margins)
                    .map(|(box_, margin)| {
                        axis_extent_of(box_, cross_axis) + margin_axis(margin, cross_axis)
                    })
                    .fold(0.0, f32::max)
            };
            let row_baseline_target = baseline_alignment(layout, main_axis)
                .then(|| baseline_target(&row_boxes, &row_margins, cross_axis));

            let mut row_cursor_main = origin_main + leading;
            for (slot, &i) in row.iter().enumerate() {
                let child = &node.children[i];
                let margin = &margins[i];
                let child_cross =
                    placed_cross(layout.cross_align, child, cross_axis, area_cross, &boxes[i]);
                let child_baseline = baseline_of(&boxes[i], cross_axis);
                set_axis_extent(&mut boxes[i], main_axis, row_main[slot]);
                set_axis_extent(&mut boxes[i], cross_axis, child_cross);
                set_axis_pos(
                    &mut boxes[i],
                    main_axis,
                    row_cursor_main + margin_axis_start(margin, main_axis),
                );
                set_axis_pos(
                    &mut boxes[i],
                    cross_axis,
                    match row_baseline_target {
                        Some(target) => row_cursor + target - child_baseline,
                        None => {
                            row_cursor
                                + cross_align_pos(
                                    layout.cross_align,
                                    child_cross + margin_axis(margin, cross_axis),
                                    row_height,
                                    margin_axis_start(margin, cross_axis),
                                )
                        }
                    },
                );
                row_cursor_main +=
                    row_main[slot] + margin_axis(margin, main_axis) + layout.gap + per_gap;
            }
            result.append(&mut drain_indices(&mut boxes, row));
            row_cursor += row_height + layout.gap;
        }
        result
    }

    // ---------- Stack：Content 尺寸推导 + FillOverlay 对齐摆放（见 006-4.2） ----------

    fn measure_stack(
        &mut self,
        node: &UiNode,
        constraints: Constraints,
        mut children: Vec<LayoutBox>,
    ) -> Result<LayoutBox, UiLayoutError> {
        let layout = node.layout.as_ref().cloned().unwrap_or_default();
        // children 已按 inner 约束测好（content 子使用；overlay 子需盒后测量，函数内重测）。
        let content_indices: Vec<usize> = node
            .children
            .iter()
            .enumerate()
            .filter(|(_, c)| {
                c.layout.as_ref().map(|l| l.stack_layer) != Some(StackLayer::FillOverlay)
            })
            .map(|(i, _)| i)
            .collect();
        let overlay_indices: Vec<usize> = node
            .children
            .iter()
            .enumerate()
            .filter(|(_, c)| {
                c.layout.as_ref().map(|l| l.stack_layer) == Some(StackLayer::FillOverlay)
            })
            .map(|(i, _)| i)
            .collect();

        // Content 子（来自参数 children，按 inner 约束测好）→ union 尺寸（含 margin）。
        let mut content_boxes: Vec<LayoutBox> = Vec::with_capacity(content_indices.len());
        let mut union_w: f32 = 0.0;
        let mut union_h: f32 = 0.0;
        for &i in &content_indices {
            // children 按树序测量（原始索引 i），content_indices 是过滤后的索引。
            let cb = std::mem::take(&mut children[i]);
            let m = margin_of(&node.children[i]);
            union_w = union_w.max(axis_extent_of(&cb, Axis::Width) + margin_axis(&m, Axis::Width));
            union_h =
                union_h.max(axis_extent_of(&cb, Axis::Height) + margin_axis(&m, Axis::Height));
            content_boxes.push(cb);
        }

        // 容器自身尺寸：声明 or Auto → Content union。
        let self_w = self.resolve_self_axis(
            node,
            Axis::Width,
            union_w,
            constraints.max_w,
            constraints.min_w,
            constraints.max_w,
        )?;
        let self_h = self.resolve_self_axis(
            node,
            Axis::Height,
            union_h,
            constraints.max_h,
            constraints.min_h,
            constraints.max_h,
        )?;
        let mut box_ = LayoutBox {
            x: 0.0,
            y: 0.0,
            w: self_w,
            h: self_h,
            first_baseline: None,
            children: Vec::new(),
        };

        let area_w = content_area(&box_, &layout, Axis::Width);
        let area_h = content_area(&box_, &layout, Axis::Height);
        let origin_x = layout.border_width + layout.padding.left;
        let origin_y = layout.border_width + layout.padding.top;

        // Content 子：叠放于内容区原点（树序即层级），带 margin 偏移。
        let mut boxes: Vec<LayoutBox> = Vec::with_capacity(node.children.len());
        for (slot, &i) in content_indices.iter().enumerate() {
            let m = margin_of(&node.children[i]);
            let mut cb = std::mem::take(&mut content_boxes[slot]);
            cb.x = origin_x + m.left;
            cb.y = origin_y + m.top;
            boxes.push(cb);
        }
        // FillOverlay 子：测量（约束 = Stack 最终盒内容区，MinMax(Fill) 基于最终盒生效）→ 对齐 + 边角偏移。
        for &i in &overlay_indices {
            let overlay_constraints = Constraints {
                min_w: 0.0,
                max_w: area_w,
                min_h: 0.0,
                max_h: area_h,
            };
            let mut cb = self.measure_inner(&node.children[i], overlay_constraints)?;
            let child = &node.children[i];
            let align = child
                .layout
                .as_ref()
                .and_then(|l| l.stack_align)
                .unwrap_or_default();
            let offset = child
                .layout
                .as_ref()
                .map(|l| l.stack_offset)
                .unwrap_or_default();
            let (x, y) = stack_align_pos(cb.w, cb.h, area_w, area_h, align, offset);
            cb.x = origin_x + x;
            cb.y = origin_y + y;
            boxes.push(cb);
        }
        box_.children = boxes;
        box_.first_baseline = propagated_baseline(&box_.children);
        Ok(box_)
    }

    // ---------- ScrollView：视口 + 宽松内容（滚动偏移由 resolve 阶段外部输入，见 006-5） ----------

    fn measure_scroll_view(
        &mut self,
        node: &UiNode,
        constraints: Constraints,
        children: Vec<LayoutBox>,
    ) -> Result<LayoutBox, UiLayoutError> {
        let layout = node.layout.as_ref().cloned().unwrap_or_default();
        // children 已按宽松约束测好（可超出视口，滚动裁剪由 emit 阶段处理）。
        let content_w = children.iter().map(|c| c.x + c.w).fold(0.0, f32::max);
        let content_h = children.iter().map(|c| c.y + c.h).fold(0.0, f32::max);
        let self_w = self.resolve_self_axis(
            node,
            Axis::Width,
            content_w,
            constraints.max_w,
            constraints.min_w,
            constraints.max_w,
        )?;
        let self_h = self.resolve_self_axis(
            node,
            Axis::Height,
            content_h,
            constraints.max_h,
            constraints.min_h,
            constraints.max_h,
        )?;
        let mut box_ = LayoutBox {
            x: 0.0,
            y: 0.0,
            w: self_w,
            h: self_h,
            first_baseline: None,
            children: Vec::new(),
        };
        // 内容按 Column 语义纵向排布（gap + margin）。
        let origin_x = layout.border_width + layout.padding.left;
        let mut cursor_y = layout.border_width + layout.padding.top;
        let mut placed = Vec::with_capacity(children.len());
        for (index, mut cb) in children.into_iter().enumerate() {
            let margin = margin_of(&node.children[index]);
            cb.x = origin_x + margin.left;
            cb.y = cursor_y + margin.top;
            cursor_y += cb.h + margin.top + margin.bottom + layout.gap;
            placed.push(cb);
        }
        box_.children = placed;
        box_.first_baseline = propagated_baseline(&box_.children);
        Ok(box_)
    }

    // ---------- 虚拟列表：定高 item 摆位（见 006-布局引擎 6） ----------

    /// 虚拟列表：item 按 `item_height + item_spacing` 定高步进摆位（业务只构建可视范围 item，
    /// 跨滚动状态由业务数据承载）；滚动偏移由 resolve 阶段经 `scroll_inputs` 平移裁剪。
    fn measure_virtual_list(
        &mut self,
        node: &UiNode,
        spec: tela_contract::VirtualListSpec,
        constraints: Constraints,
        children: Vec<LayoutBox>,
    ) -> Result<LayoutBox, UiLayoutError> {
        let layout = node.layout.as_ref().cloned().unwrap_or_default();
        let content_h = if spec.total_items == 0 {
            0.0
        } else {
            spec.total_items as f32 * spec.item_height
                + (spec.total_items - 1) as f32 * spec.item_spacing
        };
        let content_w = children.iter().map(|c| c.w).fold(0.0, f32::max);
        let self_w = self.resolve_self_axis(
            node,
            Axis::Width,
            content_w,
            constraints.max_w,
            constraints.min_w,
            constraints.max_w,
        )?;
        let self_h = self.resolve_self_axis(
            node,
            Axis::Height,
            content_h,
            constraints.max_h,
            constraints.min_h,
            constraints.max_h,
        )?;
        let mut box_ = LayoutBox {
            x: 0.0,
            y: 0.0,
            w: self_w,
            h: self_h,
            first_baseline: None,
            children: Vec::new(),
        };
        let origin_x = layout.border_width + layout.padding.left;
        let mut placed = Vec::with_capacity(children.len());
        for (index, mut cb) in children.into_iter().enumerate() {
            let margin = margin_of(&node.children[index]);
            cb.x = origin_x + margin.left;
            cb.y = (spec.first_item_index as usize + index) as f32
                * (spec.item_height + spec.item_spacing)
                + margin.top;
            placed.push(cb);
        }
        box_.children = placed;
        box_.first_baseline = propagated_baseline(&box_.children);
        Ok(box_)
    }

    /// 自身尺寸解析：声明 or Auto → fallback（内容尺寸），再区间钳制。
    fn resolve_self_axis(
        &self,
        node: &UiNode,
        axis: Axis,
        fallback: f32,
        percent_base: f32,
        min: f32,
        max: f32,
    ) -> Result<f32, UiLayoutError> {
        self.resolve_size_axis(
            node,
            axis,
            AxisSize {
                percent_base,
                fill_base: percent_base,
                auto_fallback: fallback,
                min,
                max,
            },
        )
    }

    /// 三层解析：基准层（Fixed/Percent/Auto/Fill）→ 本地 MinMax 钳制 → 父约束二次钳制。
    ///
    /// 合法尺寸区间 = 本地 `[min, max]` 与父约束 `[min, max]` 的交集；
    /// 交集为空（无法满足最小约束）返回 `UiLayoutError::MinConstraintViolation`。
    fn resolve_size_axis(
        &self,
        node: &UiNode,
        axis: Axis,
        params: AxisSize,
    ) -> Result<f32, UiLayoutError> {
        let size = match axis {
            Axis::Width => node.layout.as_ref().and_then(|l| l.width),
            Axis::Height => node.layout.as_ref().and_then(|l| l.height),
        };
        let (raw, minmax) = match size {
            None => (params.auto_fallback, None),
            Some(Size::Raw(base)) => (
                self.base_value(
                    base,
                    params.percent_base,
                    params.fill_base,
                    params.auto_fallback,
                ),
                None,
            ),
            Some(Size::Constrained(minmax)) => (
                self.base_value(
                    minmax.base,
                    params.percent_base,
                    params.fill_base,
                    params.auto_fallback,
                ),
                Some(minmax),
            ),
        };
        let local_min = minmax.and_then(|m| m.min).unwrap_or(f32::NEG_INFINITY);
        let local_max = minmax.and_then(|m| m.max).unwrap_or(f32::INFINITY);
        let lo = local_min.max(params.min);
        let hi = local_max.min(params.max);
        if lo > hi {
            return Err(UiLayoutError::MinConstraintViolation);
        }
        Ok(raw.clamp(lo, hi))
    }

    fn base_value(
        &self,
        base: BaseSize,
        percent_base: f32,
        fill_base: f32,
        auto_fallback: f32,
    ) -> f32 {
        match base {
            BaseSize::Fixed(v) => v,
            BaseSize::Percent(p) => percent_base * p,
            BaseSize::Auto => auto_fallback,
            BaseSize::Fill => fill_base,
        }
    }

    /// 测量全部子节点（同一约束）。
    fn measure_children(
        &mut self,
        node: &UiNode,
        constraints: Constraints,
    ) -> Result<Vec<LayoutBox>, UiLayoutError> {
        node.children
            .iter()
            .map(|c| self.measure_inner(c, constraints))
            .collect()
    }
}

/// 容器子节点测量约束（与 `measure_node` 的参数约定一致；Dirty 逐节点缓存复用）。
pub(crate) fn children_constraints(node: &UiNode, constraints: Constraints) -> Constraints {
    let layout = node.layout.as_ref().cloned().unwrap_or_default();
    match &node.kind {
        NodeKind::Flex | NodeKind::Stack => inner_constraints(constraints, &layout),
        NodeKind::ScrollView => Constraints {
            min_w: 0.0,
            max_w: f32::INFINITY,
            min_h: 0.0,
            max_h: f32::INFINITY,
        },
        // 虚拟列表 item 沿用父约束（定高摆位在容器内部完成）。
        NodeKind::VirtualListView(_) => constraints,
        _ => constraints,
    }
}

// ---------- 测量缓存（路线 A，见 004-7.1） ----------

/// 三层解析参数：基准分母、fill 基准、Auto 兜底与父约束区间。
#[derive(Clone, Copy)]
struct AxisSize {
    percent_base: f32,
    fill_base: f32,
    auto_fallback: f32,
    min: f32,
    max: f32,
}

/// 测量缓存 key：节点指针 + 完整约束（纯输入比对失效判定）。
type MeasureKey = (usize, u32, u32, u32, u32);

/// 单节点测量缓存：`Constraints → 自身尺寸` 查表。
///
/// key 为节点指针 + 完整约束（纯输入比对失效判定）；只缓存原语叶子测量，
/// 缓存只是输入→输出的查表加速，不引入运行时可变状态。
#[derive(Default)]
pub(crate) struct MeasureCache {
    map: HashMap<MeasureKey, (f32, f32, Option<f32>)>,
    hits: usize,
    misses: usize,
}

// ---------- 轴抽象与盒模型辅助 ----------

#[derive(Clone, Copy, PartialEq, Eq)]
enum Axis {
    Width,
    Height,
}

fn is_fill_axis(node: &UiNode, axis: Axis) -> bool {
    let size = match axis {
        Axis::Width => node.layout.as_ref().and_then(|l| l.width),
        Axis::Height => node.layout.as_ref().and_then(|l| l.height),
    };
    matches!(
        size,
        Some(Size::Raw(BaseSize::Fill))
            | Some(Size::Constrained(MinMax {
                base: BaseSize::Fill,
                ..
            }))
    )
}

/// 自身尺寸是否声明为非内容推导（Fixed/Percent/Fill），需要基于最终内容区重测子节点。
fn declared_non_auto(node: &UiNode, axis: Axis) -> bool {
    let size = match axis {
        Axis::Width => node.layout.as_ref().and_then(|l| l.width),
        Axis::Height => node.layout.as_ref().and_then(|l| l.height),
    };
    matches!(
        size,
        Some(Size::Raw(base)) if !matches!(base, BaseSize::Auto)
    ) || matches!(
        size,
        Some(Size::Constrained(minmax)) if !matches!(minmax.base, BaseSize::Auto)
    )
}

fn margins_of(node: &UiNode) -> Vec<Insets> {
    node.children.iter().map(margin_of).collect()
}

fn fill_flags(node: &UiNode, axis: Axis) -> Vec<bool> {
    node.children
        .iter()
        .map(|c| is_fill_axis(c, axis))
        .collect()
}

/// 由容器最终盒尺寸推导的子节点内容区约束（min 取 0，父 min 已在自身解析中应用）。
fn content_area_constraints(
    layout: &tela_contract::LayoutConcern,
    main_axis: Axis,
    self_main: f32,
    self_cross: f32,
) -> Constraints {
    let width = match main_axis {
        Axis::Width => self_main,
        Axis::Height => self_cross,
    };
    let height = match main_axis {
        Axis::Width => self_cross,
        Axis::Height => self_main,
    };
    Constraints {
        min_w: 0.0,
        max_w: (width - 2.0 * layout.border_width - layout.padding.left - layout.padding.right)
            .max(0.0),
        min_h: 0.0,
        max_h: (height - 2.0 * layout.border_width - layout.padding.top - layout.padding.bottom)
            .max(0.0),
    }
}

fn margin_of(node: &UiNode) -> Insets {
    node.layout.as_ref().map(|l| l.margin).unwrap_or_default()
}

/// 内容区约束：父约束减去 padding 与 border（min 不低于 0）。
fn inner_constraints(
    constraints: Constraints,
    layout: &tela_contract::LayoutConcern,
) -> Constraints {
    Constraints {
        min_w: (constraints.min_w
            - 2.0 * layout.border_width
            - layout.padding.left
            - layout.padding.right)
            .max(0.0),
        max_w: (constraints.max_w
            - 2.0 * layout.border_width
            - layout.padding.left
            - layout.padding.right)
            .max(0.0),
        min_h: (constraints.min_h
            - 2.0 * layout.border_width
            - layout.padding.top
            - layout.padding.bottom)
            .max(0.0),
        max_h: (constraints.max_h
            - 2.0 * layout.border_width
            - layout.padding.top
            - layout.padding.bottom)
            .max(0.0),
    }
}

fn axis_extent(c: &Constraints, axis: Axis) -> f32 {
    match axis {
        Axis::Width => c.max_w,
        Axis::Height => c.max_h,
    }
}

fn axis_min(c: &Constraints, axis: Axis) -> f32 {
    match axis {
        Axis::Width => c.min_w,
        Axis::Height => c.min_h,
    }
}

fn axis_max(c: &Constraints, axis: Axis) -> f32 {
    match axis {
        Axis::Width => c.max_w,
        Axis::Height => c.max_h,
    }
}

fn axis_extent_of(b: &LayoutBox, axis: Axis) -> f32 {
    match axis {
        Axis::Width => b.w,
        Axis::Height => b.h,
    }
}

fn set_axis_extent(b: &mut LayoutBox, axis: Axis, value: f32) {
    match axis {
        Axis::Width => b.w = value,
        Axis::Height => b.h = value,
    }
}

fn set_axis_pos(b: &mut LayoutBox, axis: Axis, value: f32) {
    match axis {
        Axis::Width => b.x = value,
        Axis::Height => b.y = value,
    }
}

fn margin_axis(m: &Insets, axis: Axis) -> f32 {
    match axis {
        Axis::Width => m.left + m.right,
        Axis::Height => m.top + m.bottom,
    }
}

fn margin_axis_start(m: &Insets, axis: Axis) -> f32 {
    match axis {
        Axis::Width => m.left,
        Axis::Height => m.top,
    }
}

fn margin_axis_end(m: &Insets, axis: Axis) -> f32 {
    match axis {
        Axis::Width => m.right,
        Axis::Height => m.bottom,
    }
}

fn baseline_alignment(layout: &tela_contract::LayoutConcern, main_axis: Axis) -> bool {
    layout.cross_align == CrossAlign::Baseline && main_axis == Axis::Width
}

fn baseline_of(box_: &LayoutBox, cross_axis: Axis) -> f32 {
    box_.first_baseline
        .unwrap_or_else(|| axis_extent_of(box_, cross_axis))
        .clamp(0.0, axis_extent_of(box_, cross_axis))
}

fn baseline_target(boxes: &[LayoutBox], margins: &[Insets], cross_axis: Axis) -> f32 {
    boxes
        .iter()
        .zip(margins)
        .map(|(box_, margin)| margin_axis_start(margin, cross_axis) + baseline_of(box_, cross_axis))
        .fold(0.0, f32::max)
}

fn baseline_cross_extent(boxes: &[LayoutBox], margins: &[Insets], cross_axis: Axis) -> f32 {
    let ascent = baseline_target(boxes, margins, cross_axis);
    let descent = boxes
        .iter()
        .zip(margins)
        .map(|(box_, margin)| {
            axis_extent_of(box_, cross_axis) - baseline_of(box_, cross_axis)
                + margin_axis_end(margin, cross_axis)
        })
        .fold(0.0, f32::max);
    ascent + descent
}

fn propagated_baseline(children: &[LayoutBox]) -> Option<f32> {
    children
        .iter()
        .find_map(|child| child.first_baseline.map(|baseline| child.y + baseline))
}

/// 盒内容区尺寸（减 padding/border）。
fn content_area(box_: &LayoutBox, layout: &tela_contract::LayoutConcern, axis: Axis) -> f32 {
    let extent = axis_extent_of(box_, axis);
    let inner = inner_constraints(
        Constraints {
            min_w: 0.0,
            max_w: extent,
            min_h: 0.0,
            max_h: extent,
        },
        layout,
    );
    axis_extent(&inner, axis)
}

fn content_origin(layout: &tela_contract::LayoutConcern, axis: Axis) -> f32 {
    match axis {
        Axis::Width => layout.border_width + layout.padding.left,
        Axis::Height => layout.border_width + layout.padding.top,
    }
}

/// main_align 的空闲空间分配：(行首偏移, 每 gap 额外间距)。
fn main_align_spacing(align: MainAlign, free: f32, count: usize) -> (f32, f32) {
    match align {
        MainAlign::Start => (0.0, 0.0),
        MainAlign::Center => (free / 2.0, 0.0),
        MainAlign::End => (free, 0.0),
        MainAlign::SpaceBetween => (
            0.0,
            if count > 1 {
                free / (count - 1) as f32
            } else {
                0.0
            },
        ),
        MainAlign::SpaceAround => (free / (2 * count.max(1)) as f32, free / count.max(1) as f32),
    }
}

/// 交叉轴对齐位置（相对内容区原点）。
fn cross_align_pos(
    align: CrossAlign,
    child_extent_with_margin: f32,
    area_extent: f32,
    margin_start: f32,
) -> f32 {
    match align {
        CrossAlign::Start => margin_start,
        CrossAlign::Center => {
            ((area_extent - child_extent_with_margin) / 2.0).max(0.0) + margin_start
        }
        CrossAlign::End => (area_extent - child_extent_with_margin).max(0.0) + margin_start,
        CrossAlign::Baseline => margin_start,
        CrossAlign::Stretch => margin_start,
    }
}

/// 摆放时的交叉轴尺寸：仅 `CrossAlign::Stretch` 时拉伸未声明尺寸的子节点，否则用测量值。
fn placed_cross(
    align: CrossAlign,
    child: &UiNode,
    cross_axis: Axis,
    area_cross: f32,
    box_: &LayoutBox,
) -> f32 {
    if align == CrossAlign::Stretch {
        stretch_cross(child, cross_axis, area_cross, box_)
    } else {
        axis_extent_of(box_, cross_axis)
    }
}

/// Stretch 交叉轴：子节点交叉尺寸未显式声明（None/Auto）时拉伸到内容区。
fn stretch_cross(child: &UiNode, cross_axis: Axis, area_cross: f32, box_: &LayoutBox) -> f32 {
    if stretch_implicit(child, cross_axis) {
        area_cross
    } else {
        axis_extent_of(box_, cross_axis)
    }
}

/// 交叉尺寸是否未显式声明（None/Auto），Stretch 时由容器拉伸。
fn stretch_implicit(child: &UiNode, cross_axis: Axis) -> bool {
    let declared = match cross_axis {
        Axis::Width => child.layout.as_ref().and_then(|l| l.width),
        Axis::Height => child.layout.as_ref().and_then(|l| l.height),
    };
    matches!(
        declared,
        None | Some(Size::Raw(BaseSize::Auto))
            | Some(Size::Constrained(MinMax {
                base: BaseSize::Auto,
                ..
            }))
    )
}

/// Stack `FillOverlay` 对齐摆放（见 006-4.2）。
fn stack_align_pos(
    w: f32,
    h: f32,
    area_w: f32,
    area_h: f32,
    align: StackAlign,
    offset: tela_contract::PixelOffset,
) -> (f32, f32) {
    use StackAlign::*;
    let x = match align {
        TopLeft | CenterLeft | BottomLeft => 0.0,
        TopCenter | Center | BottomCenter => (area_w - w).max(0.0) / 2.0,
        TopRight | CenterRight | BottomRight => (area_w - w).max(0.0),
    };
    let y = match align {
        TopLeft | TopCenter | TopRight => 0.0,
        CenterLeft | Center | CenterRight => (area_h - h).max(0.0) / 2.0,
        BottomLeft | BottomCenter | BottomRight => (area_h - h).max(0.0),
    };
    (x + offset.x, y + offset.y)
}

/// 按索引提取并移除子盒（保持相对位置，追加进结果）。
fn drain_indices(boxes: &mut [LayoutBox], indices: &[usize]) -> Vec<LayoutBox> {
    let mut taken: Vec<LayoutBox> = Vec::with_capacity(indices.len());
    for &i in indices {
        taken.push(std::mem::take(&mut boxes[i]));
    }
    taken
}
