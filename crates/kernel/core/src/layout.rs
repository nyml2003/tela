//! 布局引擎。
//!
//! 该模块的核心不变量是：一次 `LayoutEngine::measure` 中，每个源 `UiNode` 至多进入一次
//! 测量。容器可以分阶段调度兄弟节点，但不会用临时 clone 或修正后的约束重测子树。

use std::collections::HashMap;

use tela_contract::{
    BaseSize, Constraints, ContentConcern, CrossAlign, GridAlign, GridItemPlacement, GridSpec,
    GridTrack, Insets, LayoutBox, LayoutConcern, MinMax, NodeKind, OverlaySpec, Size, StackAlign,
    TextMeasureRequest, TextMeasurer, UiLayoutError, UiNode, VirtualListSpec,
};

/// 布局引擎抽象。
pub trait LayoutEngine {
    /// 测量节点及其子树，输出相对父盒的布局盒树。
    fn measure(
        &mut self,
        node: &UiNode,
        constraints: Constraints,
    ) -> Result<LayoutBox, UiLayoutError>;
}

/// 默认布局引擎：原语化线性布局、Stack、滚动与虚拟列表。
pub struct DefaultLayoutEngine<'a, M: TextMeasurer + ?Sized> {
    text_measurer: &'a M,
    cache: MeasureCache,
    visits: HashMap<usize, usize>,
    /// 子树指纹按 Rc 指针记忆（单次 resolve 生命周期）：共享子树只哈希一次。
    /// 引擎每次 resolve 新建，天然按遍清空——不存在跨帧地址复用风险。
    pub(crate) fingerprint_memo: HashMap<usize, (u64, bool)>,
}

/// 容器调度时对子树的唯一入口。
///
/// 普通 resolve 直接递归；Dirty resolve 在这里检查子树缓存。布局原语只通过这个接口
/// 请求子节点，因此两种路径共享同一份“最终约束后只测一次”的调度逻辑。
pub(crate) trait ChildMeasurer<M: TextMeasurer + ?Sized> {
    fn measure_child(
        &mut self,
        engine: &mut DefaultLayoutEngine<'_, M>,
        child: &UiNode,
        index: usize,
        constraints: Constraints,
    ) -> Result<LayoutBox, UiLayoutError>;

    /// 测量布局包装器内部的 child。
    ///
    /// 直接 resolve 不需要关心包装器路径；Dirty resolve 则必须保留它，避免
    /// `Stack` 的普通第 0 个 child 与第 1 个 `Overlay` 的内部第 0 个 child
    /// 落到同一个缓存 key。
    fn measure_wrapped_child(
        &mut self,
        engine: &mut DefaultLayoutEngine<'_, M>,
        _wrapper: &UiNode,
        _wrapper_index: usize,
        child: &UiNode,
        child_index: usize,
        constraints: Constraints,
    ) -> Result<LayoutBox, UiLayoutError> {
        self.measure_child(engine, child, child_index, constraints)
    }
}

struct DirectChildMeasurer;

impl<M: TextMeasurer + ?Sized> ChildMeasurer<M> for DirectChildMeasurer {
    fn measure_child(
        &mut self,
        engine: &mut DefaultLayoutEngine<'_, M>,
        child: &UiNode,
        _index: usize,
        constraints: Constraints,
    ) -> Result<LayoutBox, UiLayoutError> {
        engine.measure_with(child, constraints, self)
    }
}

impl<'a, M: TextMeasurer + ?Sized> DefaultLayoutEngine<'a, M> {
    /// 以文本度量器构造引擎。
    pub fn new(text_measurer: &'a M) -> Self {
        Self {
            text_measurer,
            cache: MeasureCache::default(),
            visits: HashMap::new(),
            fingerprint_memo: HashMap::new(),
        }
    }

    /// 清空叶子测量缓存。
    pub fn clear_cache(&mut self) {
        self.cache.map.clear();
        self.cache.hits = 0;
        self.cache.misses = 0;
    }

    /// 叶子测量缓存的命中/未命中计数。
    pub fn cache_stats(&self) -> (usize, usize) {
        (self.cache.hits, self.cache.misses)
    }

    /// 本轮 `measure` 中任一源节点的最大访问次数。用于单次测量回归测试。
    pub fn max_measure_count(&self) -> usize {
        self.visits.values().copied().max().unwrap_or(0)
    }

    /// 开始一轮新的根级测量审计。Dirty 调度会在进入根节点前调用它。
    pub(crate) fn reset_measure_audit(&mut self) {
        self.visits.clear();
    }

    fn visit(&mut self, node: &UiNode) {
        let key = node as *const UiNode as usize;
        *self.visits.entry(key).or_default() += 1;
    }
}

impl<M: TextMeasurer + ?Sized> LayoutEngine for DefaultLayoutEngine<'_, M> {
    fn measure(
        &mut self,
        node: &UiNode,
        constraints: Constraints,
    ) -> Result<LayoutBox, UiLayoutError> {
        self.reset_measure_audit();
        let mut children = DirectChildMeasurer;
        self.measure_with(node, constraints, &mut children)
    }
}

impl<'a, M: TextMeasurer + ?Sized> DefaultLayoutEngine<'a, M> {
    pub(crate) fn measure_with<P: ChildMeasurer<M>>(
        &mut self,
        node: &UiNode,
        constraints: Constraints,
        children: &mut P,
    ) -> Result<LayoutBox, UiLayoutError> {
        self.visit(node);
        validate_text_constraint_for_measurement(node)?;
        if node.kind.is_logical_container() {
            return self.measure_logical(node, constraints, children);
        }
        if node.kind.is_primitive() {
            return self.measure_leaf(node, constraints);
        }
        match &node.kind {
            NodeKind::Row => self.measure_linear(node, constraints, Axis::Width, false, children),
            NodeKind::Column => {
                self.measure_linear(node, constraints, Axis::Height, false, children)
            }
            NodeKind::BaselineRow => {
                self.measure_linear(node, constraints, Axis::Width, true, children)
            }
            NodeKind::Wrap => self.measure_wrap(node, constraints, children),
            NodeKind::Grid(spec) => self.measure_grid(node, spec, constraints, children),
            NodeKind::Frame => self.measure_frame(node, constraints, children),
            NodeKind::View => self.measure_view(node, constraints, children),
            // Expanded/Spacer/Overlay only receive their final context from the parent primitive.
            // The fallback makes malformed direct use deterministic; validation rejects it in normal trees.
            NodeKind::Expanded => self.measure_expanded_after_visit(
                node,
                None,
                constraints,
                Axis::Width,
                None,
                children,
            ),
            NodeKind::Spacer => Ok(LayoutBox::default()),
            NodeKind::Stack => self.measure_stack(node, constraints, children),
            NodeKind::Overlay(spec) => {
                self.measure_overlay_after_visit(node, None, constraints, *spec, children)
            }
            NodeKind::ScrollView => self.measure_scroll_view(node, constraints, children),
            NodeKind::VirtualListView(spec) => {
                self.measure_virtual_list(node, *spec, constraints, children)
            }
            _ => unreachable!("所有布局节点已在上方分发"),
        }
    }

    fn measure_logical<P: ChildMeasurer<M>>(
        &mut self,
        node: &UiNode,
        constraints: Constraints,
        child_measurer: &mut P,
    ) -> Result<LayoutBox, UiLayoutError> {
        let mut children = Vec::with_capacity(node.children.len());
        for (index, child) in node.children.iter().enumerate() {
            children.push(child_measurer.measure_child(self, child, index, constraints)?);
        }
        let w = children
            .iter()
            .map(|child| child.x + child.w)
            .fold(0.0, f32::max);
        let h = children
            .iter()
            .map(|child| child.y + child.h)
            .fold(0.0, f32::max);
        Ok(LayoutBox {
            x: 0.0,
            y: 0.0,
            w,
            h,
            first_baseline: propagated_baseline(&children),
            children,
        })
    }

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
        if let Some((w, h, baseline)) = self.cache.map.get(&key) {
            self.cache.hits += 1;
            return Ok(LayoutBox {
                x: 0.0,
                y: 0.0,
                w: *w,
                h: *h,
                first_baseline: *baseline,
                children: Vec::new(),
            });
        }
        self.cache.misses += 1;

        let text_metrics = match &node.content {
            Some(ContentConcern::Text(text)) => {
                Some(self.text_measurer.measure(&TextMeasureRequest {
                    text: &text.text,
                    text_style: &text.font,
                    font_size: text.font_size,
                    line_height: text.line_height,
                    max_width: Some(constraints.max_w),
                }))
            }
            _ => None,
        };
        let text_auto_height = match (&node.content, text_metrics) {
            (Some(ContentConcern::Text(text)), Some(metrics)) => node
                .layout
                .as_ref()
                .and_then(|layout| layout.text_constraint)
                .and_then(|constraint| constraint.max_lines)
                .map(|max_lines| metrics.height.min(text.line_height * max_lines as f32))
                .unwrap_or(metrics.height),
            _ => 0.0,
        };
        let w = self.resolve_size_axis(
            node,
            Axis::Width,
            AxisSize {
                percent_base: constraints.max_w,
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
                auto_fallback: text_auto_height,
                min: constraints.min_h,
                max: constraints.max_h,
            },
        )?;
        let baseline = text_metrics.map(|metrics| metrics.first_baseline.clamp(0.0, h));
        self.cache.map.insert(key, (w, h, baseline));
        Ok(LayoutBox {
            x: 0.0,
            y: 0.0,
            w,
            h,
            first_baseline: baseline,
            children: Vec::new(),
        })
    }

    /// Row / Column / BaselineRow。
    fn measure_linear<P: ChildMeasurer<M>>(
        &mut self,
        node: &UiNode,
        constraints: Constraints,
        main_axis: Axis,
        baseline: bool,
        child_measurer: &mut P,
    ) -> Result<LayoutBox, UiLayoutError> {
        let cross_axis = main_axis.other();
        let layout = layout_of(node);
        let known_main = self.declared_extent(node, main_axis, constraints)?;
        let known_cross = self.declared_extent(node, cross_axis, constraints)?;
        let mut child_constraints = inner_constraints(constraints, &layout);
        if let Some(extent) = known_main {
            set_axis_range(
                &mut child_constraints,
                main_axis,
                0.0,
                (extent - axis_insets(&layout, main_axis)).max(0.0),
            );
        }
        if let Some(extent) = known_cross {
            set_axis_range(
                &mut child_constraints,
                cross_axis,
                0.0,
                (extent - axis_insets(&layout, cross_axis)).max(0.0),
            );
        }
        let main_definite = known_main.is_some_and(f32::is_finite);
        let mut boxes: Vec<Option<LayoutBox>> = vec![None; node.children.len()];
        let mut deferred = Vec::new();

        // 第一阶段：自然项目只接受最终的外部约束一次。
        for (index, child) in node.children.iter().enumerate() {
            match &child.kind {
                NodeKind::Expanded if main_definite => deferred.push(index),
                NodeKind::Spacer if main_definite => deferred.push(index),
                NodeKind::Expanded => {
                    boxes[index] = Some(self.measure_expanded(
                        child,
                        index,
                        child_constraints,
                        main_axis,
                        None,
                        child_measurer,
                    )?);
                }
                NodeKind::Spacer => {
                    self.visit(child);
                    boxes[index] = Some(LayoutBox::default());
                }
                _ => {
                    boxes[index] =
                        Some(child_measurer.measure_child(self, child, index, child_constraints)?)
                }
            }
        }

        // 第二阶段：只给尚未测量的分配项提供已确定的主轴份额。
        if main_definite {
            let area_main = (known_main.unwrap_or(0.0) - axis_insets(&layout, main_axis)).max(0.0);
            let fixed = boxes
                .iter()
                .enumerate()
                .filter_map(|(index, box_)| box_.as_ref().map(|box_| (index, box_)))
                .map(|(index, box_)| {
                    axis_extent_of(box_, main_axis)
                        + margin_axis(&margin_of(&node.children[index]), main_axis)
                })
                .sum::<f32>();
            let deferred_margins = deferred
                .iter()
                .map(|index| margin_axis(&margin_of(&node.children[*index]), main_axis))
                .sum::<f32>();
            let gaps = layout.gap * node.children.len().saturating_sub(1) as f32;
            let share = (area_main - fixed - deferred_margins - gaps).max(0.0)
                / deferred.len().max(1) as f32;
            for index in deferred {
                let child = &node.children[index];
                boxes[index] = Some(match &child.kind {
                    NodeKind::Expanded => self.measure_expanded(
                        child,
                        index,
                        child_constraints,
                        main_axis,
                        Some(share),
                        child_measurer,
                    )?,
                    NodeKind::Spacer => {
                        self.visit(child);
                        spacer_box(main_axis, share)
                    }
                    _ => unreachable!("仅分配项会被延后"),
                });
            }
        }

        let mut boxes: Vec<LayoutBox> = boxes
            .into_iter()
            .map(|box_| box_.expect("布局形状已在构建期校验"))
            .collect();
        let margins: Vec<Insets> = node.children.iter().map(|child| margin_of(child)).collect();
        let content_main = boxes
            .iter()
            .zip(&margins)
            .map(|(box_, margin)| axis_extent_of(box_, main_axis) + margin_axis(margin, main_axis))
            .sum::<f32>()
            + layout.gap * node.children.len().saturating_sub(1) as f32;
        let content_cross = if baseline {
            baseline_cross_extent(&boxes, &margins, cross_axis)
        } else {
            boxes
                .iter()
                .zip(&margins)
                .map(|(box_, margin)| {
                    axis_extent_of(box_, cross_axis) + margin_axis(margin, cross_axis)
                })
                .fold(0.0, f32::max)
        };
        let self_main = known_main.unwrap_or(self.resolve_self_axis(
            node,
            main_axis,
            content_main + axis_insets(&layout, main_axis),
            constraints,
        )?);
        let self_cross = known_cross.unwrap_or(self.resolve_self_axis(
            node,
            cross_axis,
            content_cross + axis_insets(&layout, cross_axis),
            constraints,
        )?);
        let mut result = empty_box();
        set_axis_extent(&mut result, main_axis, self_main);
        set_axis_extent(&mut result, cross_axis, self_cross);
        let area_main = content_area(&result, &layout, main_axis);
        let area_cross = content_area(&result, &layout, cross_axis);
        let origin_main = content_origin(&layout, main_axis);
        let origin_cross = content_origin(&layout, cross_axis);
        let baseline_target = baseline.then(|| baseline_target(&boxes, &margins, cross_axis));
        let mut cursor = origin_main;
        for (index, box_) in boxes.iter_mut().enumerate() {
            let margin = margins[index];
            set_axis_pos(
                box_,
                main_axis,
                cursor + margin_axis_start(&margin, main_axis),
            );
            let cross = if let Some(target) = baseline_target {
                margin_axis_start(&margin, cross_axis) + target
                    - (margin_axis_start(&margin, cross_axis) + baseline_of(box_, cross_axis))
            } else {
                cross_align_pos(
                    layout.cross_align,
                    axis_extent_of(box_, cross_axis) + margin_axis(&margin, cross_axis),
                    area_cross,
                    margin_axis_start(&margin, cross_axis),
                )
            };
            set_axis_pos(box_, cross_axis, origin_cross + cross.max(0.0));
            cursor +=
                axis_extent_of(box_, main_axis) + margin_axis(&margin, main_axis) + layout.gap;
        }
        let _ = area_main; // area_main documents the allocation source and aids debug inspection.
        result.first_baseline = propagated_baseline(&boxes);
        result.children = boxes;
        Ok(result)
    }

    fn measure_expanded<P: ChildMeasurer<M>>(
        &mut self,
        node: &UiNode,
        wrapper_index: usize,
        constraints: Constraints,
        main_axis: Axis,
        allocation: Option<f32>,
        child_measurer: &mut P,
    ) -> Result<LayoutBox, UiLayoutError> {
        self.visit(node);
        self.measure_expanded_after_visit(
            node,
            Some(wrapper_index),
            constraints,
            main_axis,
            allocation,
            child_measurer,
        )
    }

    fn measure_expanded_after_visit<P: ChildMeasurer<M>>(
        &mut self,
        node: &UiNode,
        wrapper_index: Option<usize>,
        constraints: Constraints,
        main_axis: Axis,
        allocation: Option<f32>,
        child_measurer: &mut P,
    ) -> Result<LayoutBox, UiLayoutError> {
        let child = node.children.first().expect("Expanded 形状已校验");
        let mut child_constraints = constraints;
        if let Some(allocation) = allocation {
            set_axis_range(&mut child_constraints, main_axis, 0.0, allocation);
        }
        let mut child_box = match wrapper_index {
            Some(wrapper_index) => child_measurer.measure_wrapped_child(
                self,
                node,
                wrapper_index,
                child,
                0,
                child_constraints,
            )?,
            None => child_measurer.measure_child(self, child, 0, child_constraints)?,
        };
        child_box.x = 0.0;
        child_box.y = 0.0;
        let mut box_ = empty_box();
        set_axis_extent(
            &mut box_,
            main_axis,
            allocation.unwrap_or_else(|| axis_extent_of(&child_box, main_axis)),
        );
        set_axis_extent(
            &mut box_,
            main_axis.other(),
            axis_extent_of(&child_box, main_axis.other()),
        );
        box_.first_baseline = child_box.first_baseline;
        box_.children = vec![child_box];
        Ok(box_)
    }

    fn measure_wrap<P: ChildMeasurer<M>>(
        &mut self,
        node: &UiNode,
        constraints: Constraints,
        child_measurer: &mut P,
    ) -> Result<LayoutBox, UiLayoutError> {
        let layout = layout_of(node);
        let known_w = self.declared_extent(node, Axis::Width, constraints)?;
        let known_h = self.declared_extent(node, Axis::Height, constraints)?;
        let mut child_constraints = inner_constraints(constraints, &layout);
        if let Some(width) = known_w {
            set_axis_range(
                &mut child_constraints,
                Axis::Width,
                0.0,
                (width - axis_insets(&layout, Axis::Width)).max(0.0),
            );
        }
        if let Some(height) = known_h {
            set_axis_range(
                &mut child_constraints,
                Axis::Height,
                0.0,
                (height - axis_insets(&layout, Axis::Height)).max(0.0),
            );
        }
        let mut boxes = Vec::with_capacity(node.children.len());
        for (index, child) in node.children.iter().enumerate() {
            boxes.push(child_measurer.measure_child(self, child, index, child_constraints)?);
        }
        let wrap_width = known_w
            .map(|width| (width - axis_insets(&layout, Axis::Width)).max(0.0))
            .unwrap_or(child_constraints.max_w);
        let origin_x = content_origin(&layout, Axis::Width);
        let origin_y = content_origin(&layout, Axis::Height);
        let mut x = origin_x;
        let mut y = origin_y;
        let mut line_width = 0.0;
        let mut line_height = 0.0;
        let mut max_line_width: f32 = 0.0;
        for (index, box_) in boxes.iter_mut().enumerate() {
            let margin = margin_of(&node.children[index]);
            let item_width = box_.w + margin_axis(&margin, Axis::Width);
            let item_height = box_.h + margin_axis(&margin, Axis::Height);
            if line_width > 0.0 && line_width + layout.gap + item_width > wrap_width {
                max_line_width = max_line_width.max(line_width);
                x = origin_x;
                y += line_height + layout.gap;
                line_width = 0.0;
                line_height = 0.0;
            }
            if line_width > 0.0 {
                x += layout.gap;
                line_width += layout.gap;
            }
            box_.x = x + margin.left;
            box_.y = y + margin.top;
            x += item_width;
            line_width += item_width;
            line_height = line_height.max(item_height);
        }
        max_line_width = max_line_width.max(line_width);
        let content_h = if boxes.is_empty() {
            0.0
        } else {
            y - origin_y + line_height
        };
        let self_w = known_w.unwrap_or(self.resolve_self_axis(
            node,
            Axis::Width,
            max_line_width + axis_insets(&layout, Axis::Width),
            constraints,
        )?);
        let self_h = known_h.unwrap_or(self.resolve_self_axis(
            node,
            Axis::Height,
            content_h + axis_insets(&layout, Axis::Height),
            constraints,
        )?);
        Ok(LayoutBox {
            x: 0.0,
            y: 0.0,
            w: self_w,
            h: self_h,
            first_baseline: propagated_baseline(&boxes),
            children: boxes,
        })
    }

    /// Grid：轨道先由容器的最终尺寸确定，再以每个 item 的最终单元格约束测量一次。
    ///
    /// 轨道不支持隐式内容尺寸，所以不会出现“量完 child 才发现轨道变大”的回溯测量。
    fn measure_grid<P: ChildMeasurer<M>>(
        &mut self,
        node: &UiNode,
        spec: &GridSpec,
        constraints: Constraints,
        child_measurer: &mut P,
    ) -> Result<LayoutBox, UiLayoutError> {
        let layout = layout_of(node);
        let known_w = self.declared_extent(node, Axis::Width, constraints)?;
        let known_h = self.declared_extent(node, Axis::Height, constraints)?;
        let inset_w = axis_insets(&layout, Axis::Width);
        let inset_h = axis_insets(&layout, Axis::Height);
        let fallback_w = grid_preferred_extent(
            &spec.columns,
            spec.column_gap,
            known_w
                .map(|extent| (extent - inset_w).max(0.0))
                .or_else(|| {
                    constraints
                        .max_w
                        .is_finite()
                        .then_some((constraints.max_w - inset_w).max(0.0))
                }),
        )? + inset_w;
        let fallback_h = grid_preferred_extent(
            &spec.rows,
            spec.row_gap,
            known_h
                .map(|extent| (extent - inset_h).max(0.0))
                .or_else(|| {
                    constraints
                        .max_h
                        .is_finite()
                        .then_some((constraints.max_h - inset_h).max(0.0))
                }),
        )? + inset_h;
        let self_w = known_w.unwrap_or(self.resolve_self_axis(
            node,
            Axis::Width,
            fallback_w,
            constraints,
        )?);
        let self_h = known_h.unwrap_or(self.resolve_self_axis(
            node,
            Axis::Height,
            fallback_h,
            constraints,
        )?);
        let columns =
            resolve_grid_tracks(&spec.columns, spec.column_gap, (self_w - inset_w).max(0.0))?;
        let rows = resolve_grid_tracks(&spec.rows, spec.row_gap, (self_h - inset_h).max(0.0))?;
        let placements = grid_placements(node, columns.len(), rows.len())?;
        let column_offsets = grid_track_offsets(&columns, spec.column_gap);
        let row_offsets = grid_track_offsets(&rows, spec.row_gap);
        let origin_x = content_origin(&layout, Axis::Width);
        let origin_y = content_origin(&layout, Axis::Height);
        let mut children = Vec::with_capacity(node.children.len());

        for (index, child) in node.children.iter().enumerate() {
            let placement = placements[index];
            let column = usize::from(placement.column);
            let row = usize::from(placement.row);
            let width = grid_span_extent(
                &columns,
                spec.column_gap,
                column,
                usize::from(placement.column_span),
            )?;
            let height =
                grid_span_extent(&rows, spec.row_gap, row, usize::from(placement.row_span))?;
            let margin = margin_of(child);
            let available_w = (width - margin_axis(&margin, Axis::Width)).max(0.0);
            let available_h = (height - margin_axis(&margin, Axis::Height)).max(0.0);
            let mut child_constraints = Constraints {
                min_w: 0.0,
                max_w: available_w,
                min_h: 0.0,
                max_h: available_h,
            };
            if placement.justify_self == GridAlign::Stretch {
                child_constraints.min_w = available_w;
            }
            if placement.align_self == GridAlign::Stretch {
                child_constraints.min_h = available_h;
            }
            let mut box_ = child_measurer.measure_child(self, child, index, child_constraints)?;
            box_.x = origin_x
                + column_offsets[column]
                + margin.left
                + grid_align_offset(placement.justify_self, box_.w, available_w);
            box_.y = origin_y
                + row_offsets[row]
                + margin.top
                + grid_align_offset(placement.align_self, box_.h, available_h);
            children.push(box_);
        }

        Ok(LayoutBox {
            x: 0.0,
            y: 0.0,
            w: self_w,
            h: self_h,
            first_baseline: propagated_baseline(&children),
            children,
        })
    }

    /// 通用盒模型容器：0..1 个内容子节点（空 View = 纯装饰块）。
    fn measure_view<P: ChildMeasurer<M>>(
        &mut self,
        node: &UiNode,
        constraints: Constraints,
        child_measurer: &mut P,
    ) -> Result<LayoutBox, UiLayoutError> {
        let layout = layout_of(node);
        let known_w = self.declared_extent(node, Axis::Width, constraints)?;
        let known_h = self.declared_extent(node, Axis::Height, constraints)?;
        let mut child_constraints = inner_constraints(constraints, &layout);
        if let Some(width) = known_w {
            set_axis_range(
                &mut child_constraints,
                Axis::Width,
                0.0,
                (width - axis_insets(&layout, Axis::Width)).max(0.0),
            );
        }
        if let Some(height) = known_h {
            set_axis_range(
                &mut child_constraints,
                Axis::Height,
                0.0,
                (height - axis_insets(&layout, Axis::Height)).max(0.0),
            );
        }
        let child = match node.children.first() {
            Some(child) => {
                let mut measured =
                    child_measurer.measure_child(self, child, 0, child_constraints)?;
                measured.x = content_origin(&layout, Axis::Width);
                measured.y = content_origin(&layout, Axis::Height);
                measured
            }
            None => LayoutBox::default(),
        };
        let self_w = known_w.unwrap_or(self.resolve_self_axis(
            node,
            Axis::Width,
            child.w + axis_insets(&layout, Axis::Width),
            constraints,
        )?);
        let self_h = known_h.unwrap_or(self.resolve_self_axis(
            node,
            Axis::Height,
            child.h + axis_insets(&layout, Axis::Height),
            constraints,
        )?);
        Ok(LayoutBox {
            x: 0.0,
            y: 0.0,
            w: self_w,
            h: self_h,
            first_baseline: child.first_baseline.map(|baseline| child.y + baseline),
            children: if node.children.is_empty() {
                Vec::new()
            } else {
                vec![child]
            },
        })
    }

    fn measure_frame<P: ChildMeasurer<M>>(
        &mut self,
        node: &UiNode,
        constraints: Constraints,
        child_measurer: &mut P,
    ) -> Result<LayoutBox, UiLayoutError> {
        let layout = layout_of(node);
        let known_w = self.declared_extent(node, Axis::Width, constraints)?;
        let known_h = self.declared_extent(node, Axis::Height, constraints)?;
        let mut child_constraints = inner_constraints(constraints, &layout);
        if let Some(width) = known_w {
            set_axis_range(
                &mut child_constraints,
                Axis::Width,
                0.0,
                (width - axis_insets(&layout, Axis::Width)).max(0.0),
            );
        }
        if let Some(height) = known_h {
            set_axis_range(
                &mut child_constraints,
                Axis::Height,
                0.0,
                (height - axis_insets(&layout, Axis::Height)).max(0.0),
            );
        }
        let mut child = child_measurer.measure_child(
            self,
            node.children.first().expect("Frame 形状已校验"),
            0,
            child_constraints,
        )?;
        child.x = content_origin(&layout, Axis::Width);
        child.y = content_origin(&layout, Axis::Height);
        let self_w = known_w.unwrap_or(self.resolve_self_axis(
            node,
            Axis::Width,
            child.w + axis_insets(&layout, Axis::Width),
            constraints,
        )?);
        let self_h = known_h.unwrap_or(self.resolve_self_axis(
            node,
            Axis::Height,
            child.h + axis_insets(&layout, Axis::Height),
            constraints,
        )?);
        Ok(LayoutBox {
            x: 0.0,
            y: 0.0,
            w: self_w,
            h: self_h,
            first_baseline: child.first_baseline.map(|baseline| child.y + baseline),
            children: vec![child],
        })
    }

    fn measure_stack<P: ChildMeasurer<M>>(
        &mut self,
        node: &UiNode,
        constraints: Constraints,
        child_measurer: &mut P,
    ) -> Result<LayoutBox, UiLayoutError> {
        let layout = layout_of(node);
        let known_w = self.declared_extent(node, Axis::Width, constraints)?;
        let known_h = self.declared_extent(node, Axis::Height, constraints)?;
        let mut content_constraints = inner_constraints(constraints, &layout);
        if let Some(width) = known_w {
            set_axis_range(
                &mut content_constraints,
                Axis::Width,
                0.0,
                (width - axis_insets(&layout, Axis::Width)).max(0.0),
            );
        }
        if let Some(height) = known_h {
            set_axis_range(
                &mut content_constraints,
                Axis::Height,
                0.0,
                (height - axis_insets(&layout, Axis::Height)).max(0.0),
            );
        }
        let mut children: Vec<LayoutBox> = vec![LayoutBox::default(); node.children.len()];
        let mut content_w: f32 = 0.0;
        let mut content_h: f32 = 0.0;
        for (index, child) in node.children.iter().enumerate() {
            if matches!(&child.kind, NodeKind::Overlay(_)) {
                continue;
            }
            let mut box_ = child_measurer.measure_child(self, child, index, content_constraints)?;
            let margin = margin_of(child);
            content_w = content_w.max(box_.w + margin_axis(&margin, Axis::Width));
            content_h = content_h.max(box_.h + margin_axis(&margin, Axis::Height));
            box_.x = content_origin(&layout, Axis::Width) + margin.left;
            box_.y = content_origin(&layout, Axis::Height) + margin.top;
            children[index] = box_;
        }
        let self_w = known_w.unwrap_or(self.resolve_self_axis(
            node,
            Axis::Width,
            content_w + axis_insets(&layout, Axis::Width),
            constraints,
        )?);
        let self_h = known_h.unwrap_or(self.resolve_self_axis(
            node,
            Axis::Height,
            content_h + axis_insets(&layout, Axis::Height),
            constraints,
        )?);
        let area_w = (self_w - axis_insets(&layout, Axis::Width)).max(0.0);
        let area_h = (self_h - axis_insets(&layout, Axis::Height)).max(0.0);
        let overlay_constraints = Constraints {
            min_w: 0.0,
            max_w: area_w,
            min_h: 0.0,
            max_h: area_h,
        };
        for (index, child) in node.children.iter().enumerate() {
            let NodeKind::Overlay(spec) = &child.kind else {
                continue;
            };
            let mut box_ =
                self.measure_overlay(child, index, overlay_constraints, *spec, child_measurer)?;
            let (x, y) = stack_align_pos(box_.w, box_.h, area_w, area_h, spec.align, spec.offset);
            box_.x = content_origin(&layout, Axis::Width) + x;
            box_.y = content_origin(&layout, Axis::Height) + y;
            children[index] = box_;
        }
        Ok(LayoutBox {
            x: 0.0,
            y: 0.0,
            w: self_w,
            h: self_h,
            first_baseline: propagated_baseline(&children),
            children,
        })
    }

    fn measure_overlay<P: ChildMeasurer<M>>(
        &mut self,
        node: &UiNode,
        wrapper_index: usize,
        constraints: Constraints,
        spec: OverlaySpec,
        child_measurer: &mut P,
    ) -> Result<LayoutBox, UiLayoutError> {
        self.visit(node);
        self.measure_overlay_after_visit(
            node,
            Some(wrapper_index),
            constraints,
            spec,
            child_measurer,
        )
    }

    fn measure_overlay_after_visit<P: ChildMeasurer<M>>(
        &mut self,
        node: &UiNode,
        wrapper_index: Option<usize>,
        constraints: Constraints,
        spec: OverlaySpec,
        child_measurer: &mut P,
    ) -> Result<LayoutBox, UiLayoutError> {
        let mut child_constraints = constraints;
        if spec.fill_width {
            child_constraints.min_w = constraints.max_w;
        }
        if spec.fill_height {
            child_constraints.min_h = constraints.max_h;
        }
        let child_node = node.children.first().expect("Overlay 形状已校验");
        let mut child = match wrapper_index {
            Some(wrapper_index) => child_measurer.measure_wrapped_child(
                self,
                node,
                wrapper_index,
                child_node,
                0,
                child_constraints,
            )?,
            None => child_measurer.measure_child(self, child_node, 0, child_constraints)?,
        };
        child.x = 0.0;
        child.y = 0.0;
        let w = if spec.fill_width {
            constraints.max_w
        } else {
            child.w
        };
        let h = if spec.fill_height {
            constraints.max_h
        } else {
            child.h
        };
        Ok(LayoutBox {
            x: 0.0,
            y: 0.0,
            w,
            h,
            first_baseline: child.first_baseline,
            children: vec![child],
        })
    }

    fn measure_scroll_view<P: ChildMeasurer<M>>(
        &mut self,
        node: &UiNode,
        constraints: Constraints,
        child_measurer: &mut P,
    ) -> Result<LayoutBox, UiLayoutError> {
        let layout = layout_of(node);
        let known_w = self.declared_extent(node, Axis::Width, constraints)?;
        let known_h = self.declared_extent(node, Axis::Height, constraints)?;
        let loose = Constraints {
            min_w: 0.0,
            max_w: f32::INFINITY,
            min_h: 0.0,
            max_h: f32::INFINITY,
        };
        let mut children = Vec::with_capacity(node.children.len());
        for (index, child) in node.children.iter().enumerate() {
            children.push(child_measurer.measure_child(self, child, index, loose)?);
        }
        let content_w = children
            .iter()
            .map(|child| child.x + child.w)
            .fold(0.0, f32::max);
        let content_h = children
            .iter()
            .map(|child| child.y + child.h)
            .fold(0.0, f32::max);
        let self_w =
            known_w.unwrap_or(self.resolve_self_axis(node, Axis::Width, content_w, constraints)?);
        let self_h = known_h.unwrap_or(self.resolve_self_axis(
            node,
            Axis::Height,
            content_h,
            constraints,
        )?);
        let mut cursor = content_origin(&layout, Axis::Height);
        for (index, box_) in children.iter_mut().enumerate() {
            let margin = margin_of(&node.children[index]);
            box_.x = content_origin(&layout, Axis::Width) + margin.left;
            box_.y = cursor + margin.top;
            cursor += box_.h + margin_axis(&margin, Axis::Height) + layout.gap;
        }
        Ok(LayoutBox {
            x: 0.0,
            y: 0.0,
            w: self_w,
            h: self_h,
            first_baseline: propagated_baseline(&children),
            children,
        })
    }

    fn measure_virtual_list<P: ChildMeasurer<M>>(
        &mut self,
        node: &UiNode,
        spec: VirtualListSpec,
        constraints: Constraints,
        child_measurer: &mut P,
    ) -> Result<LayoutBox, UiLayoutError> {
        let layout = layout_of(node);
        let known_w = self.declared_extent(node, Axis::Width, constraints)?;
        let known_h = self.declared_extent(node, Axis::Height, constraints)?;
        let mut child_constraints = inner_constraints(constraints, &layout);
        if let Some(width) = known_w {
            set_axis_range(
                &mut child_constraints,
                Axis::Width,
                0.0,
                (width - axis_insets(&layout, Axis::Width)).max(0.0),
            );
        }
        if let Some(height) = known_h {
            set_axis_range(
                &mut child_constraints,
                Axis::Height,
                0.0,
                (height - axis_insets(&layout, Axis::Height)).max(0.0),
            );
        }
        let mut children = Vec::with_capacity(node.children.len());
        for (index, child) in node.children.iter().enumerate() {
            let mut box_ = child_measurer.measure_child(self, child, index, child_constraints)?;
            let margin = margin_of(child);
            box_.x = content_origin(&layout, Axis::Width) + margin.left;
            box_.y = (spec.first_item_index as usize + index) as f32
                * (spec.item_height + spec.item_spacing)
                + margin.top;
            children.push(box_);
        }
        let content_h = if spec.total_items == 0 {
            0.0
        } else {
            spec.total_items as f32 * spec.item_height
                + (spec.total_items - 1) as f32 * spec.item_spacing
        };
        let content_w = children
            .iter()
            .map(|child| child.x + child.w)
            .fold(0.0, f32::max);
        let self_w =
            known_w.unwrap_or(self.resolve_self_axis(node, Axis::Width, content_w, constraints)?);
        let self_h = known_h.unwrap_or(self.resolve_self_axis(
            node,
            Axis::Height,
            content_h,
            constraints,
        )?);
        Ok(LayoutBox {
            x: 0.0,
            y: 0.0,
            w: self_w,
            h: self_h,
            first_baseline: propagated_baseline(&children),
            children,
        })
    }

    fn declared_extent(
        &self,
        node: &UiNode,
        axis: Axis,
        constraints: Constraints,
    ) -> Result<Option<f32>, UiLayoutError> {
        if is_auto_axis(node, axis) {
            return Ok(None);
        }
        self.resolve_self_axis(node, axis, 0.0, constraints)
            .map(Some)
    }

    fn resolve_self_axis(
        &self,
        node: &UiNode,
        axis: Axis,
        fallback: f32,
        constraints: Constraints,
    ) -> Result<f32, UiLayoutError> {
        self.resolve_size_axis(
            node,
            axis,
            AxisSize {
                percent_base: axis_max(&constraints, axis),
                auto_fallback: fallback,
                min: axis_min(&constraints, axis),
                max: axis_max(&constraints, axis),
            },
        )
    }

    fn resolve_size_axis(
        &self,
        node: &UiNode,
        axis: Axis,
        params: AxisSize,
    ) -> Result<f32, UiLayoutError> {
        let size = axis_size(node, axis);
        let (raw, minmax) = match size {
            None => (params.auto_fallback, None),
            Some(Size::Raw(base)) => (
                base_value(base, params.percent_base, params.auto_fallback),
                None,
            ),
            Some(Size::Constrained(minmax)) => (
                base_value(minmax.base, params.percent_base, params.auto_fallback),
                Some(minmax),
            ),
        };
        let local_min = minmax
            .and_then(|value| value.min)
            .unwrap_or(f32::NEG_INFINITY);
        let local_max = minmax.and_then(|value| value.max).unwrap_or(f32::INFINITY);
        let lo = local_min.max(params.min);
        let hi = local_max.min(params.max);
        if lo > hi {
            return Err(UiLayoutError::MinConstraintViolation);
        }
        Ok(raw.clamp(lo, hi))
    }
}

#[derive(Default)]
pub(crate) struct MeasureCache {
    map: HashMap<MeasureKey, (f32, f32, Option<f32>)>,
    hits: usize,
    misses: usize,
}

type MeasureKey = (usize, u32, u32, u32, u32);

#[derive(Clone, Copy)]
struct AxisSize {
    percent_base: f32,
    auto_fallback: f32,
    min: f32,
    max: f32,
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum Axis {
    Width,
    Height,
}

impl Axis {
    fn other(self) -> Self {
        match self {
            Self::Width => Self::Height,
            Self::Height => Self::Width,
        }
    }
}

fn layout_of(node: &UiNode) -> LayoutConcern {
    node.layout.clone().unwrap_or_default()
}

/// `UiTree::new` 之外直接调用布局引擎时，仍保持文本约束的同一失败语义。
fn validate_text_constraint_for_measurement(node: &UiNode) -> Result<(), UiLayoutError> {
    let Some(constraint) = node
        .layout
        .as_ref()
        .and_then(|layout| layout.text_constraint)
    else {
        return Ok(());
    };
    if !matches!(node.kind, NodeKind::Text) || !constraint.is_valid() {
        return Err(UiLayoutError::InvalidTextConstraint);
    }
    Ok(())
}

fn axis_size(node: &UiNode, axis: Axis) -> Option<Size> {
    let layout = node.layout.as_ref()?;
    match axis {
        Axis::Width => layout.width,
        Axis::Height => layout.height,
    }
}

fn is_auto_axis(node: &UiNode, axis: Axis) -> bool {
    matches!(
        axis_size(node, axis),
        None | Some(Size::Raw(BaseSize::Auto))
            | Some(Size::Constrained(MinMax {
                base: BaseSize::Auto,
                ..
            }))
    )
}

fn base_value(base: BaseSize, percent_base: f32, auto_fallback: f32) -> f32 {
    match base {
        BaseSize::Fixed(value) => value,
        BaseSize::Percent(ratio) => percent_base * ratio,
        BaseSize::Auto => auto_fallback,
    }
}

fn empty_box() -> LayoutBox {
    LayoutBox::default()
}

fn spacer_box(main_axis: Axis, share: f32) -> LayoutBox {
    let mut box_ = empty_box();
    set_axis_extent(&mut box_, main_axis, share);
    box_
}

fn inner_constraints(constraints: Constraints, layout: &LayoutConcern) -> Constraints {
    Constraints {
        min_w: (constraints.min_w - axis_insets(layout, Axis::Width)).max(0.0),
        max_w: (constraints.max_w - axis_insets(layout, Axis::Width)).max(0.0),
        min_h: (constraints.min_h - axis_insets(layout, Axis::Height)).max(0.0),
        max_h: (constraints.max_h - axis_insets(layout, Axis::Height)).max(0.0),
    }
}

fn axis_insets(layout: &LayoutConcern, axis: Axis) -> f32 {
    let padding = match axis {
        Axis::Width => layout.padding.left + layout.padding.right,
        Axis::Height => layout.padding.top + layout.padding.bottom,
    };
    padding + 2.0 * layout.border_width
}

fn content_area(box_: &LayoutBox, layout: &LayoutConcern, axis: Axis) -> f32 {
    (axis_extent_of(box_, axis) - axis_insets(layout, axis)).max(0.0)
}

fn content_origin(layout: &LayoutConcern, axis: Axis) -> f32 {
    match axis {
        Axis::Width => layout.border_width + layout.padding.left,
        Axis::Height => layout.border_width + layout.padding.top,
    }
}

fn axis_min(constraints: &Constraints, axis: Axis) -> f32 {
    match axis {
        Axis::Width => constraints.min_w,
        Axis::Height => constraints.min_h,
    }
}

fn axis_max(constraints: &Constraints, axis: Axis) -> f32 {
    match axis {
        Axis::Width => constraints.max_w,
        Axis::Height => constraints.max_h,
    }
}

fn set_axis_range(constraints: &mut Constraints, axis: Axis, min: f32, max: f32) {
    match axis {
        Axis::Width => {
            constraints.min_w = min;
            constraints.max_w = max;
        }
        Axis::Height => {
            constraints.min_h = min;
            constraints.max_h = max;
        }
    }
}

fn axis_extent_of(box_: &LayoutBox, axis: Axis) -> f32 {
    match axis {
        Axis::Width => box_.w,
        Axis::Height => box_.h,
    }
}

fn set_axis_extent(box_: &mut LayoutBox, axis: Axis, value: f32) {
    match axis {
        Axis::Width => box_.w = value,
        Axis::Height => box_.h = value,
    }
}

fn set_axis_pos(box_: &mut LayoutBox, axis: Axis, value: f32) {
    match axis {
        Axis::Width => box_.x = value,
        Axis::Height => box_.y = value,
    }
}

fn margin_of(node: &UiNode) -> Insets {
    node.layout
        .as_ref()
        .map(|layout| layout.margin)
        .unwrap_or_default()
}

fn margin_axis(margin: &Insets, axis: Axis) -> f32 {
    match axis {
        Axis::Width => margin.left + margin.right,
        Axis::Height => margin.top + margin.bottom,
    }
}

fn margin_axis_start(margin: &Insets, axis: Axis) -> f32 {
    match axis {
        Axis::Width => margin.left,
        Axis::Height => margin.top,
    }
}

fn propagated_baseline(children: &[LayoutBox]) -> Option<f32> {
    children
        .iter()
        .find_map(|child| child.first_baseline.map(|baseline| child.y + baseline))
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
                + (margin_axis(margin, cross_axis) - margin_axis_start(margin, cross_axis))
        })
        .fold(0.0, f32::max);
    ascent + descent
}

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
    }
}

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

fn grid_preferred_extent(
    tracks: &[GridTrack],
    gap: f32,
    available: Option<f32>,
) -> Result<f32, UiLayoutError> {
    validate_grid_tracks(tracks, gap)?;
    let fixed = tracks
        .iter()
        .map(|track| match track {
            GridTrack::Fixed(value) => *value,
            GridTrack::Flex(_) => 0.0,
        })
        .sum::<f32>();
    let minimum = fixed + gap * tracks.len().saturating_sub(1) as f32;
    if tracks
        .iter()
        .any(|track| matches!(track, GridTrack::Flex(_)))
    {
        Ok(available.unwrap_or(minimum).max(minimum))
    } else {
        Ok(minimum)
    }
}

fn resolve_grid_tracks(
    tracks: &[GridTrack],
    gap: f32,
    available: f32,
) -> Result<Vec<f32>, UiLayoutError> {
    validate_grid_tracks(tracks, gap)?;
    let fixed = tracks
        .iter()
        .map(|track| match track {
            GridTrack::Fixed(value) => *value,
            GridTrack::Flex(_) => 0.0,
        })
        .sum::<f32>();
    let total_weight = tracks
        .iter()
        .map(|track| match track {
            GridTrack::Fixed(_) => 0.0,
            GridTrack::Flex(weight) => *weight,
        })
        .sum::<f32>();
    let free = (available - fixed - gap * tracks.len().saturating_sub(1) as f32).max(0.0);
    Ok(tracks
        .iter()
        .map(|track| match track {
            GridTrack::Fixed(value) => *value,
            GridTrack::Flex(weight) => free * *weight / total_weight,
        })
        .collect())
}

fn validate_grid_tracks(tracks: &[GridTrack], gap: f32) -> Result<(), UiLayoutError> {
    if tracks.is_empty()
        || !gap.is_finite()
        || gap < 0.0
        || tracks.iter().any(|track| match track {
            GridTrack::Fixed(value) => !value.is_finite() || *value < 0.0,
            GridTrack::Flex(weight) => !weight.is_finite() || *weight <= 0.0,
        })
    {
        return Err(UiLayoutError::InvalidGrid);
    }
    Ok(())
}

fn grid_track_offsets(tracks: &[f32], gap: f32) -> Vec<f32> {
    let mut offsets = Vec::with_capacity(tracks.len());
    let mut cursor = 0.0;
    for track in tracks {
        offsets.push(cursor);
        cursor += *track + gap;
    }
    offsets
}

fn grid_span_extent(
    tracks: &[f32],
    gap: f32,
    start: usize,
    span: usize,
) -> Result<f32, UiLayoutError> {
    if span == 0 || start >= tracks.len() || start.saturating_add(span) > tracks.len() {
        return Err(UiLayoutError::InvalidGrid);
    }
    Ok(tracks[start..start + span].iter().sum::<f32>() + gap * (span - 1) as f32)
}

fn grid_align_offset(align: GridAlign, child_extent: f32, available: f32) -> f32 {
    match align {
        GridAlign::Start | GridAlign::Stretch => 0.0,
        GridAlign::Center => ((available - child_extent) / 2.0).max(0.0),
        GridAlign::End => (available - child_extent).max(0.0),
    }
}

fn grid_placements(
    node: &UiNode,
    columns: usize,
    rows: usize,
) -> Result<Vec<GridItemPlacement>, UiLayoutError> {
    if columns == 0 || rows == 0 {
        return Err(UiLayoutError::InvalidGrid);
    }
    let mut occupied = vec![false; columns * rows];
    for child in &node.children {
        if let Some(placement) = child.layout.as_ref().and_then(|layout| layout.grid_item) {
            occupy_layout_grid_cells(&mut occupied, columns, rows, placement)?;
        }
    }
    let mut placements = Vec::with_capacity(node.children.len());
    for child in &node.children {
        let placement =
            if let Some(placement) = child.layout.as_ref().and_then(|layout| layout.grid_item) {
                placement
            } else {
                let Some(index) = occupied.iter().position(|occupied| !occupied) else {
                    return Err(UiLayoutError::InvalidGrid);
                };
                occupied[index] = true;
                GridItemPlacement::at((index % columns) as u16, (index / columns) as u16)
            };
        placements.push(placement);
    }
    Ok(placements)
}

fn occupy_layout_grid_cells(
    occupied: &mut [bool],
    columns: usize,
    rows: usize,
    placement: GridItemPlacement,
) -> Result<(), UiLayoutError> {
    let column = usize::from(placement.column);
    let row = usize::from(placement.row);
    let column_span = usize::from(placement.column_span);
    let row_span = usize::from(placement.row_span);
    if column_span == 0
        || row_span == 0
        || column >= columns
        || row >= rows
        || column.saturating_add(column_span) > columns
        || row.saturating_add(row_span) > rows
    {
        return Err(UiLayoutError::InvalidGrid);
    }
    for y in row..row + row_span {
        for x in column..column + column_span {
            let cell = &mut occupied[y * columns + x];
            if *cell {
                return Err(UiLayoutError::InvalidGrid);
            }
            *cell = true;
        }
    }
    Ok(())
}
