//! 每帧构建期的不可变 TypeId 能力作用域。

use std::{
    any::{Any, TypeId},
    collections::HashMap,
    rc::Rc,
    sync::Arc,
};

use crate::{ViewBuildError, ViewResult, ViewSite};

/// 供 `provide` 写入一个词法 Context 的已拥有能力值。
///
/// 单线程 `Rc` 体系（与 `Signal` 一致）：作用域可以携带 `Signal<T>` / `Computed<T>`，
/// 使 provide/inject 成为响应式边的另一种源端发现方式（001 §2）——但注意
/// 恒定值与信号都可注入；**会变的值必须以信号节点注入**，普通值注入即契约恒定。
#[derive(Clone)]
pub struct ProvidedValue {
    type_id: TypeId,
    type_name: &'static str,
    value: Rc<dyn Any>,
}

impl ProvidedValue {
    /// 以显式目标类型保存一个能力值。
    ///
    /// 泛型参数由宏的 `@provide(value: Type)` 直接给出，因此表达式类型不依赖使用处
    /// 的隐式推导。
    pub fn new<T: 'static>(value: T) -> Self {
        Self {
            type_id: TypeId::of::<T>(),
            type_name: std::any::type_name::<T>(),
            value: Rc::new(value),
        }
    }
}

/// 不可变的词法能力作用域。
///
/// 每个 `ui!` 块可创建一个 child scope。子 scope 可以遮蔽父项，但同一层级重复
/// 提供相同 `TypeId` 会返回结构化错误。
pub struct ViewContext {
    parent: Option<Arc<Self>>,
    entries: HashMap<TypeId, ProvidedValue>,
}

impl ViewContext {
    /// 创建没有父层和能力值的根作用域。
    pub fn root() -> Arc<Self> {
        Arc::new(Self {
            parent: None,
            entries: HashMap::new(),
        })
    }

    /// 在 `parent` 上创建一个包含本层能力值的不可变子作用域。
    pub fn child(
        parent: Arc<Self>,
        values: impl IntoIterator<Item = ProvidedValue>,
        site: ViewSite,
    ) -> ViewResult<Arc<Self>> {
        let mut entries = HashMap::new();
        for value in values {
            if entries.insert(value.type_id, value.clone()).is_some() {
                return Err(ViewBuildError::DuplicateProvider {
                    type_name: value.type_name,
                    site,
                });
            }
        }
        Ok(Arc::new(Self {
            parent: Some(parent),
            entries,
        }))
    }

    /// 从当前作用域开始向父链查询一个能力值。
    pub fn inject<T: 'static>(&self, site: ViewSite) -> ViewResult<&T> {
        let type_id = TypeId::of::<T>();
        let mut current = Some(self);
        while let Some(scope) = current {
            if let Some(value) = scope.entries.get(&type_id) {
                return value
                    .value
                    .downcast_ref::<T>()
                    .ok_or(ViewBuildError::MissingProvider {
                        type_name: std::any::type_name::<T>(),
                        site,
                    });
            }
            current = scope.parent.as_deref();
        }
        Err(ViewBuildError::MissingProvider {
            type_name: std::any::type_name::<T>(),
            site,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::{ProvidedValue, ViewContext};
    use crate::{ViewBuildError, ViewSite};

    fn site() -> ViewSite {
        ViewSite::new("context.rs", 1, 1)
    }

    #[test]
    fn child_scopes_shadow_parent_values_without_mutating_it() {
        let root = ViewContext::root();
        let parent =
            ViewContext::child(root, [ProvidedValue::new::<u32>(1)], site()).expect("parent scope");
        let child = ViewContext::child(parent.clone(), [ProvidedValue::new::<u32>(2)], site())
            .expect("child scope");

        assert_eq!(*parent.inject::<u32>(site()).expect("parent value"), 1);
        assert_eq!(*child.inject::<u32>(site()).expect("child value"), 2);
    }

    #[test]
    fn duplicate_provider_is_a_build_error() {
        let result = ViewContext::child(
            ViewContext::root(),
            [ProvidedValue::new::<u32>(1), ProvidedValue::new::<u32>(2)],
            site(),
        );
        let Err(error) = result else {
            panic!("duplicate provider must fail");
        };
        assert!(matches!(error, ViewBuildError::DuplicateProvider { .. }));
    }
}
