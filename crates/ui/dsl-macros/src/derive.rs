//! `#[derive(DslComponent)]`：struct 字段即 Props，派生宏生成 Props 镜像与
//! `render` 脚手架（inject/provide/watch 能力 + 类型 assert）。

use proc_macro2::TokenStream as TokenStream2;
use quote::{format_ident, quote};
use syn::{
    Data, DeriveInput, Error, Expr, Field, Fields, GenericArgument, Ident, Path, PathArguments,
    Result, Type,
};

use crate::dsl_path;

/// `#[prop(...)]` / `#[inject]` / `#[provide]` / `#[watch]` / `#[bind(...)]` 字段语义。
#[derive(Clone, Copy, Debug, PartialEq)]
enum FieldKind {
    /// 普通 prop：`..Default` 兜底，组装 `unwrap_or_default()`（T: Default assert）。
    Plain,
    /// `#[prop(default = expr)]`：组装 `unwrap_or(expr)`。
    Defaulted,
    /// `#[prop(option)]`：字段类型必须是 `Option<U>`，镜像原样。
    Option,
    /// `#[inject]`：从 Context 注入（T: Clone assert）。
    Inject,
    /// `#[provide]`：压入子作用域（T: Clone + 'static）。
    Provide,
    /// `#[watch(key = "...")]`：订阅 Signal（类型必须 `Signal<T>`）。
    Watch,
    /// `#[bind(paint = path)]` 或 `#[bind(layout = path)]`：静态呈现绑定（类型必须
    /// `Signal<T>`）。它不触发组件重装配。
    Bind,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum BindKind {
    Paint,
    Layout,
}

struct BindingSpec {
    kind: BindKind,
    apply: Path,
    value_ty: Type,
}

struct FieldSpec {
    ident: Ident,
    ty: Type,
    kind: FieldKind,
    /// `#[inject]` 叠加在 `#[watch]` 字段上：props 显式传入优先，否则从作用域
    /// 解析 Signal/Computed（inject 边化：provide 的响应式节点直达注入点）。
    watch_inject: bool,
    default_expr: Option<Expr>,
    binding: Option<BindingSpec>,
}

pub fn expand_derive(input: DeriveInput) -> Result<TokenStream2> {
    let name = input.ident.clone();
    let fields = match &input.data {
        Data::Struct(data) => match &data.fields {
            Fields::Named(fields) => fields.named.clone(),
            _ => {
                return Err(Error::new_spanned(
                    &input,
                    "#[derive(DslComponent)] requires a struct with named fields",
                ));
            }
        },
        _ => {
            return Err(Error::new_spanned(
                &input,
                "#[derive(DslComponent)] only supports structs",
            ));
        }
    };

    let mut specs = Vec::new();
    for field in fields.iter() {
        specs.push(parse_field(field)?);
    }

    // 契约（001 §2）：derive 组件的动态数据输入只能是图上的边——`#[watch]` 字段
    // （Signal/Computed，可叠加 `#[inject]` 从作用域解析）、`#[bind]` Signal 边，或
    // 身份用的 `key`。普通值 Props 仍不是 derive 的动态数据通道。
    //
    // `#[inject]` / `#[provide]` 是唯一的额外例外：它们传递的是显式 Context capability，
    // 不是一个由宏猜测的响应式值。它们在父候选重装配时必须重新解析，因此不会参与
    // derive retained 的普通 memo hit；但一次已经提交的 retained 子树可以在自己的
    // 显式 Signal 边变脏时使用原 capability 快照独立重入。
    for spec in &specs {
        if !matches!(
            spec.kind,
            FieldKind::Watch | FieldKind::Bind | FieldKind::Inject | FieldKind::Provide
        ) && spec.ident != "key"
        {
            return Err(Error::new_spanned(
                spec.ty.clone(),
                format!(
                    "derive 组件字段 '{}' 只允许 #[watch]（Signal/Computed）、\
                     #[bind(paint = path)] / #[bind(layout = path)]（Signal）、\
                     #[inject] / #[provide] Context capability 或 `key`；\
                     动态数据用 Signal/Computed 传递，普通值写进 view 体或手写 UiSpec",
                    spec.ident
                ),
            ));
        }
    }

    // `#[tela(tag = "...")]` 别名不支持（033 定稿：默认标签名 = struct 名）。
    let props_name = format_ident!("{}Props", name);
    let identity_key = specs
        .iter()
        .find(|spec| spec.ident == "key")
        .map(|spec| {
            let value = if is_option_type(&spec.ty) && spec.kind != FieldKind::Option {
                quote!(props.key.clone().flatten())
            } else {
                quote!(props.key.clone())
            };
            quote! {
                fn identity_key(props: &Self::Props) -> Option<String> {
                    #value
                }
            }
        })
        .unwrap_or_default();

    // ---- Props 镜像字段 ----
    let props_fields = specs.iter().map(|spec| {
        let ident = &spec.ident;
        let field_ty = &spec.ty;
        match spec.kind {
            // #[prop(option)] 字段已是 Option<U>，镜像原样。
            FieldKind::Option => quote!(pub #ident: #field_ty,),
            _ => quote!(pub #ident: Option<#field_ty>,),
        }
    });

    // ---- 组装表达式（纯借用：可重复执行，供快照再构造一次实例）----
    let assembly: Vec<_> = specs
        .iter()
        .map(|spec| {
            let ident = &spec.ident;
            match spec.kind {
                FieldKind::Plain => quote!(#ident: props.#ident.clone().unwrap_or_default(),),
                FieldKind::Defaulted => {
                    let default = spec.default_expr.as_ref().expect("defaulted has expr");
                    quote!(#ident: props.#ident.clone().unwrap_or(#default),)
                }
                FieldKind::Option => quote!(#ident: props.#ident.clone(),),
                FieldKind::Inject => {
                    let binding = format_ident!("__tela_inject_{ident}");
                    quote!(#ident: #binding.clone(),)
                }
                FieldKind::Provide => {
                    let binding = format_ident!("__tela_provide_{ident}");
                    quote!(#ident: #binding.clone(),)
                }
                FieldKind::Watch => {
                    let binding = format_ident!("__tela_watch_{ident}");
                    quote!(#ident: #binding.clone(),)
                }
                FieldKind::Bind => {
                    let binding = format_ident!("__tela_bind_{ident}");
                    quote!(#ident: #binding.clone(),)
                }
            }
        })
        .collect();

    // ---- inject 绑定 ----
    let inject_code = specs.iter().filter_map(|spec| {
        if spec.kind != FieldKind::Inject {
            return None;
        }
        let ident = &spec.ident;
        let ty = &spec.ty;
        let binding = format_ident!("__tela_inject_{ident}");
        Some(quote! {
            let #binding = match props.#ident {
                Some(value) => value,
                None => __tela_build.current_scope().inject::<#ty>(__tela_site)?.clone(),
            };
        })
    });

    // ---- watch 绑定 ----
    // ---- watch 绑定：缺失时整帧返回错误（不 panic）；#[inject] 叠加时从作用域解析 ----
    let dsl = dsl_path();
    let watch_code = specs.iter().filter_map(|spec| {
        if spec.kind != FieldKind::Watch {
            return None;
        }
        let ident = &spec.ident;
        let ty = &spec.ty;
        let binding = format_ident!("__tela_watch_{ident}");
        let fallback = if spec.watch_inject {
            // inject 边化：props 显式传入优先，否则从作用域解析 Signal/Computed——
            // provide 的响应式节点越过中间节点直达注入点（001 §2）。
            quote! {
                __tela_build
                    .current_scope()
                    .inject::<#ty>(__tela_site)?
                    .clone()
            }
        } else {
            quote! {
                return Err(#dsl::ViewBuildError::MissingRequiredProp {
                    name: stringify!(#ident),
                    site: __tela_site,
                })
            }
        };
        Some(quote! {
            let #binding = match props.#ident {
                Some(value) => value,
                None => #fallback,
            };
        })
    });
    let watch_handles = specs
        .iter()
        .filter(|spec| spec.kind == FieldKind::Watch)
        .map(|spec| {
            let ident = &spec.ident;
            let binding = format_ident!("__tela_watch_{ident}");
            quote!(__tela_build.watch_source(&#binding, __tela_site))
        });

    // ---- static presentation bindings ----
    // `#[bind]` is deliberately separate from `#[watch]`: it records a function-pointer edge
    // from one read-only Signal to this component's own output root, but never reruns `view`.
    let bind_code = specs.iter().filter_map(|spec| {
        if spec.kind != FieldKind::Bind {
            return None;
        }
        let ident = &spec.ident;
        let binding = format_ident!("__tela_bind_{ident}");
        Some(quote! {
            let #binding = match props.#ident {
                Some(value) => value,
                None => return Err(#dsl::ViewBuildError::MissingRequiredProp {
                    name: stringify!(#ident),
                    site: __tela_site,
                }),
            };
        })
    });

    // Retained re-entry receives the previous component instance directly, so it must rebuild
    // the local bindings that the normal props/inject path created before entering `view`.
    let memo_watch_bindings = specs
        .iter()
        .filter(|spec| spec.kind == FieldKind::Watch)
        .map(|spec| {
            let ident = &spec.ident;
            let binding = format_ident!("__tela_watch_{ident}");
            quote!(let #binding = __tela_inst.#ident.clone();)
        });
    let memo_provide_bindings = specs
        .iter()
        .filter(|spec| spec.kind == FieldKind::Provide)
        .map(|spec| {
            let ident = &spec.ident;
            let binding = format_ident!("__tela_provide_{ident}");
            quote!(let #binding = __tela_inst.#ident.clone();)
        });

    // ---- provide 字段：缺失时整帧返回错误（不 panic）----
    let provide_code = specs.iter().filter_map(|spec| {
        if spec.kind != FieldKind::Provide {
            return None;
        }
        let ident = &spec.ident;
        let binding = format_ident!("__tela_provide_{ident}");
        Some(quote! {
            let #binding = match props.#ident {
                Some(value) => value,
                None => return Err(#dsl::ViewBuildError::MissingRequiredProp {
                    name: stringify!(#ident),
                    site: __tela_site,
                }),
            };
        })
    });
    let provided_values = specs
        .iter()
        .filter(|spec| spec.kind == FieldKind::Provide)
        .map(|spec| {
            let ty = &spec.ty;
            let ident = &spec.ident;
            let binding = format_ident!("__tela_provide_{ident}");
            // `view` needs the value through the child scope while the retained snapshot keeps
            // its own component field. Context is a read-only capability, so the scope gets an
            // owned clone rather than consuming the Props-side binding.
            quote!(#dsl::ProvidedValue::new::<#ty>(#binding.clone()))
        });

    // ---- 类型 assert：仅定义检查函数并在未执行的 __check 中引用（编译期强制）----
    // 装配为借用式（clone + unwrap），所有字段需要 Clone；Plain 另需 Default 兜底。
    let assert_calls: Vec<_> = specs
        .iter()
        .map(|spec| {
            let ty = &spec.ty;
            let clone = quote!(__assert_clone::<#ty>(););
            match spec.kind {
                FieldKind::Plain => quote!(__assert_default::<#ty>(); #clone),
                _ => clone,
            }
        })
        .collect();

    let binding_assert_calls = specs.iter().filter_map(|spec| {
        let binding = spec.binding.as_ref()?;
        let value_ty = &binding.value_ty;
        Some(quote!(__assert_clone::<#value_ty>();))
    });

    let binding_snapshot_name = format_ident!("__Tela{}BindingSnapshot", name);
    let binding_table_name = format_ident!("__TELA_{}_BINDINGS", name);
    let binding_slots_name = format_ident!("__TELA_{}_BINDING_SLOTS", name);
    let binding_specs = specs
        .iter()
        .filter_map(|spec| spec.binding.as_ref().map(|binding| (spec, binding)))
        .collect::<Vec<_>>();
    let binding_snapshot_fields = binding_specs.iter().map(|(spec, _)| {
        let ident = &spec.ident;
        let ty = &spec.ty;
        quote!(#ident: #ty,)
    });
    let binding_snapshot_values = binding_specs.iter().map(|(spec, _)| {
        let ident = &spec.ident;
        quote!(#ident: __tela_inst.#ident.clone(),)
    });
    let binding_sources = binding_specs.iter().map(|(spec, binding)| {
        let ident = &spec.ident;
        let value_ty = &binding.value_ty;
        let source = format_ident!("__tela_{}_{}_binding_source", name, ident);
        quote! {
            #[doc(hidden)]
            #[allow(non_snake_case)]
            fn #source(component: &#binding_snapshot_name) -> &#dsl::Signal<#value_ty> {
                &component.#ident
            }
        }
    });
    let binding_slots = binding_specs.iter().map(|(spec, binding)| {
        let ident = &spec.ident;
        let value_ty = &binding.value_ty;
        let source = format_ident!("__tela_{}_{}_binding_source", name, ident);
        let slot = format_ident!("__TELA_{}_{}_BINDING_SLOT", name, ident);
        let apply = &binding.apply;
        let constructor = match binding.kind {
            BindKind::Paint => quote!(#dsl::BindingSlot::paint),
            BindKind::Layout => quote!(#dsl::BindingSlot::layout),
        };
        quote! {
            #[doc(hidden)]
            #[allow(non_upper_case_globals)]
            static #slot: #dsl::BindingSlot<#binding_snapshot_name, #value_ty, #dsl::NodePresentation> =
                #constructor(#source, #apply);
        }
    });
    let binding_slot_refs = binding_specs.iter().map(|(spec, _)| {
        let ident = &spec.ident;
        let slot = format_ident!("__TELA_{}_{}_BINDING_SLOT", name, ident);
        quote!( &#slot )
    });
    let binding_support = if binding_specs.is_empty() {
        quote!()
    } else {
        let binding_count = binding_specs.len();
        quote! {
            #[doc(hidden)]
            #[derive(Clone)]
            struct #binding_snapshot_name {
                #(#binding_snapshot_fields)*
            }

            #(#binding_sources)*
            #(#binding_slots)*

            #[doc(hidden)]
            #[allow(non_upper_case_globals)]
            static #binding_slots_name: [&dyn #dsl::BindingSlotDyn<#binding_snapshot_name, #dsl::NodePresentation>; #binding_count] = [
                #(#binding_slot_refs),*
            ];

            #[doc(hidden)]
            #[allow(non_upper_case_globals)]
            static #binding_table_name: #dsl::StaticBindingTable<#binding_snapshot_name, #dsl::NodePresentation> =
                #dsl::StaticBindingTable::new(&#binding_slots_name);
        }
    };
    let binding_attach = if binding_specs.is_empty() {
        quote!()
    } else {
        quote! {
            __tela_out = __tela_out.attach_static_presentation_binding(
                #binding_snapshot_name {
                    #(#binding_snapshot_values)*
                },
                &#binding_table_name,
                __tela_site,
            );
        }
    };

    // ---- render 主体 ----
    let has_provide = specs.iter().any(|spec| spec.kind == FieldKind::Provide);
    let view_call = if has_provide {
        quote! {
            __tela_build.with_scope(
                vec![#(#provided_values),*],
                __tela_site,
                |__tela_build| {
                    __tela_inst.view(__tela_build, &__tela_children)
                },
            )
        }
    } else {
        quote!({ __tela_inst.view(__tela_build, &__tela_children) })
    };
    let watch_attach = if specs.iter().any(|spec| spec.kind == FieldKind::Watch) {
        quote! {
            __tela_out = __tela_out.attach_watches(vec![#(#watch_handles),*]);
        }
    } else {
        quote!()
    };

    let props_struct = quote! {
        #[doc(hidden)]
        #[derive(Clone, Default)]
        pub struct #props_name {
            #(#props_fields)*
        }
    };

    // ---- retained 求值语义（默认，001 §2）：入边无脏 → 不重求值 ----
    // 命中判定 = 上次实例快照的纯身份比较（watch 字段比 SignalId，key 已含于身份）。
    // children 只有被 view 显式消费后才物化为候选快照。命中不会执行调用栈闭包；定点
    // 重入把同一组 Rc 节点、watch 边、可克隆 HostInput route blueprint 和动画 scope
    // 作为单次 Children 槽位交还 view，由它决定是否继续消费。
    // 组件自身的 Output / HostInput 路由和动画调度同样由运行时作为候选 blueprint 保存并
    // 重装，保持候选事务边界。
    let memo_snapshot_name = format_ident!("__Tela{}MemoSnapshot", name);
    let memo_field_matches = specs
        .iter()
        .filter(|spec| matches!(spec.kind, FieldKind::Watch | FieldKind::Bind))
        .map(|spec| {
            let ident = &spec.ident;
            quote!(self.#ident.id() == cached.component.#ident.id())
        });
    let memo_matches_impl = quote! {
        const _: () = {
            impl #name {
                #[doc(hidden)]
                fn __tela_memo_matches<__TelaAction: 'static>(
                    &self,
                    cached: &dyn ::core::any::Any,
                ) -> bool {
                    cached
                        .downcast_ref::<#memo_snapshot_name<__TelaAction>>()
                        .is_some_and(|cached| true #(&& #memo_field_matches)*)
                }
            }
        };
    };
    // Context capability is an explicit lexical input, but it has neither a signal identity nor
    // a value-comparison contract. A parent reassembly must therefore resolve it again instead
    // of claiming a retained hit based only on the component's Signal fields. We still record a
    // retained snapshot below: a descendant may independently re-enter against the capability
    // snapshot that belongs to the currently committed parent tree.
    let memo_hit_is_safe = specs
        .iter()
        .all(|spec| matches!(spec.kind, FieldKind::Watch | FieldKind::Bind) || spec.ident == "key");
    let memo_render = quote! {
        let __tela_memo = __tela_build.memo_enabled();
        if __tela_memo && #memo_hit_is_safe {
            if let Some(__tela_cached) =
                __tela_build.memo_hit(|cached| {
                    __tela_inst.__tela_memo_matches::<__TelaAction>(cached)
                })
            {
                // 命中：缓存输出已携带 watch 声明，直接拼回，跳过 view 与重复 attach。
                return Ok(__tela_cached);
            }
        }
        let mut __tela_out = #view_call?;
        #watch_attach
        #binding_attach
        if __tela_memo {
            if let Some(__tela_retained_children) = __tela_children.retained_snapshot() {
                // 记录上次实例快照（自包含 retained element：全 watch 句柄 + 坐标 + view）。
                // 快照由绑定重新构造（句柄 clone = Rc 递增），不要求组件结构体 Clone。
                // 必须发生在 watch attach 之后：缓存条目要携带组件自身的订阅，
                // 否则脏检查看不到它的 scope，signal 变化会被误命中。
                __tela_build.memo_record(
                    #memo_snapshot_name::<__TelaAction> {
                        component: #name { #(#assembly)* },
                        marker: ::core::marker::PhantomData,
                    },
                    __tela_retained_children,
                    &__tela_out,
                    #name::__tela_memo_reenter::<__TelaAction>,
                    __tela_site,
                );
            } else {
                __tela_build.memo_forget_current();
            }
        }
        Ok(__tela_out)
    };

    let impl_block = quote! {
        #[doc(hidden)]
        struct #memo_snapshot_name<__TelaAction: 'static> {
            component: #name,
            marker: ::core::marker::PhantomData<__TelaAction>,
        }

        impl #name {
            #[doc(hidden)]
            fn __tela_memo_reenter<__TelaAction: 'static>(
                __tela_build: &mut #dsl::ViewBuild<__TelaAction>,
                __tela_cached: ::std::rc::Rc<dyn ::core::any::Any>,
                __tela_site: #dsl::ViewSite,
            ) -> #dsl::ViewResult<#dsl::ViewOutput<__TelaAction>> {
                let __tela_snapshot = __tela_cached
                    .downcast_ref::<#memo_snapshot_name<__TelaAction>>()
                    .expect("retained evaluator and snapshot component type must agree");
                let __tela_inst = &__tela_snapshot.component;
                let __tela_children = __tela_build.retained_children_slot();
                #(#memo_watch_bindings)*
                #(#memo_provide_bindings)*
                // Independent retained re-entry begins below the original lexical parent. The
                // build context restores this component's existing private Output scope so any
                // nested component still routes its Output to the same logical owner.
                let mut __tela_out = __tela_build
                    .with_retained_output_scope::<(), _>(|__tela_build| #view_call)?;
                #watch_attach
                #binding_attach
                if let Some(__tela_retained_children) = __tela_children.retained_snapshot() {
                    __tela_build.memo_record_erased(
                        __tela_cached,
                        __tela_retained_children,
                        &__tela_out,
                        #name::__tela_memo_reenter::<__TelaAction>,
                        __tela_site,
                    );
                } else {
                    __tela_build.memo_forget_current();
                }
                Ok(__tela_out)
            }
        }

        impl #dsl::DslComponent for #name {
            type UiSpec<__TelaAction: 'static> = Self;
        }

        impl<__TelaAction: 'static> #dsl::UiSpec<__TelaAction> for #name {
            type Props = #props_name;
            type State = ();
            type Event = ();
            type Output = ();

            #identity_key

            fn assemble<'__tela_children>(
                __tela_context: &mut #dsl::ComponentAssembleContext<'_, __TelaAction>,
                props: Self::Props,
                _state: &Self::State,
                __tela_children: #dsl::Children<'__tela_children, __TelaAction>,
            ) -> #dsl::ViewResult<#dsl::ViewOutput<__TelaAction>> {
                let __tela_site = __tela_context.site();
                let __tela_build = __tela_context.build();
                #(#inject_code)*
                #(#watch_code)*
                #(#bind_code)*
                #(#provide_code)*
                let __tela_inst = #name {
                    #(#assembly)*
                };
                #memo_render
            }
        }

        #memo_matches_impl

        #[doc(hidden)]
        const _: () = {
            fn __assert_default<T: Default>() {}
            fn __assert_clone<T: Clone>() {}
            fn __check() {
                #(#assert_calls)*
                #(#binding_assert_calls)*
            }
        };
    };

    let mut output = TokenStream2::new();
    output.extend(props_struct);
    output.extend(binding_support);
    output.extend(impl_block);

    Ok(output)
}

/// 解析一个字段的语义（默认 Plain）。
fn parse_field(field: &Field) -> Result<FieldSpec> {
    let ident = field
        .ident
        .clone()
        .ok_or_else(|| Error::new_spanned(field, "component fields must be named"))?;
    let ty = field.ty.clone();
    let mut kind = FieldKind::Plain;
    let mut watch_inject = false;
    let mut default_expr = None;
    let mut binding = None;

    for attr in &field.attrs {
        if !attr.path().is_ident("prop") {
            if attr.path().is_ident("inject") {
                // `#[inject]` 叠加在 `#[watch]` 上：作用域解析响应式节点（边化 inject）。
                if kind == FieldKind::Watch {
                    watch_inject = true;
                } else {
                    set_kind(&mut kind, FieldKind::Inject, attr, &ident)?;
                }
                continue;
            }
            if attr.path().is_ident("provide") {
                set_kind(&mut kind, FieldKind::Provide, attr, &ident)?;
                continue;
            }
            if attr.path().is_ident("watch") {
                if kind == FieldKind::Inject {
                    kind = FieldKind::Watch;
                    watch_inject = true;
                } else {
                    set_kind(&mut kind, FieldKind::Watch, attr, &ident)?;
                }
                match &attr.meta {
                    syn::Meta::Path(_) => {}
                    other => {
                        return Err(Error::new_spanned(
                            other,
                            "#[watch] takes no arguments; it subscribes the Signal field and rebuilds the component on change",
                        ));
                    }
                }
                continue;
            }
            if attr.path().is_ident("bind") {
                set_kind(&mut kind, FieldKind::Bind, attr, &ident)?;
                let mut parsed = None;
                attr.parse_nested_meta(|meta| {
                    let kind = if meta.path.is_ident("paint") {
                        BindKind::Paint
                    } else if meta.path.is_ident("layout") {
                        BindKind::Layout
                    } else {
                        return Err(meta.error(
                            "unknown #[bind(...)] option; expected paint = function_path or layout = function_path",
                        ));
                    };
                    if parsed.is_some() {
                        return Err(meta.error(
                            "#[bind(...)] accepts exactly one of paint = function_path or layout = function_path",
                        ));
                    }
                    let apply = meta.value()?.parse::<Path>()?;
                    parsed = Some((kind, apply));
                    Ok(())
                })?;
                let Some((kind, apply)) = parsed else {
                    return Err(Error::new_spanned(
                        attr,
                        "#[bind(...)] requires paint = function_path or layout = function_path",
                    ));
                };
                let Some(value_ty) = signal_value_type(&ty) else {
                    return Err(Error::new_spanned(
                        &ty,
                        "#[bind(...)] field must have type Signal<T>",
                    ));
                };
                binding = Some(BindingSpec {
                    kind,
                    apply,
                    value_ty,
                });
                continue;
            }
            // 非 DSL 属性（derive/其他）忽略
            continue;
        }
        attr.parse_nested_meta(|meta| {
            if meta.path.is_ident("default") {
                set_kind(&mut kind, FieldKind::Defaulted, attr, &ident)?;
                default_expr = Some(meta.value()?.parse()?);
                return Ok(());
            }
            if meta.path.is_ident("option") {
                set_kind(&mut kind, FieldKind::Option, attr, &ident)?;
                return Ok(());
            }
            Err(meta.error("unknown #[prop(...)] option; supported: default = expr, option"))
        })?;
    }

    // 类型校验：option 字段必须是 Option<U>；watch 字段必须是 Signal<T> 或 Computed<T>。
    if kind == FieldKind::Option && !is_option_type(&ty) {
        return Err(Error::new_spanned(
            &ty,
            "#[prop(option)] field must have type Option<U>",
        ));
    }
    if kind == FieldKind::Watch
        && !is_named_generic(&ty, "Signal")
        && !is_named_generic(&ty, "Computed")
    {
        return Err(Error::new_spanned(
            &ty,
            "#[watch] field must have type Signal<T> or Computed<T>",
        ));
    }

    Ok(FieldSpec {
        ident,
        ty,
        kind,
        watch_inject,
        default_expr,
        binding,
    })
}

fn set_kind(
    slot: &mut FieldKind,
    new: FieldKind,
    attr: &syn::Attribute,
    ident: &Ident,
) -> Result<()> {
    if *slot != FieldKind::Plain {
        return Err(Error::new_spanned(
            attr,
            format!(
                "field '{}' has conflicting DSL attributes (inject/provide/watch/bind/prop are exclusive)",
                ident
            ),
        ));
    }
    *slot = new;
    Ok(())
}

fn is_option_type(ty: &Type) -> bool {
    is_named_generic(ty, "Option")
}

/// Returns `T` for a direct `Signal<T>` field type.
///
/// A static presentation slot needs `&Signal<T>` as a function-pointer source. `Computed<T>` is
/// intentionally not accepted here until it has the same erased subscription surface; falling
/// back to `#[watch]` remains explicit and sound.
fn signal_value_type(ty: &Type) -> Option<Type> {
    let Type::Path(path) = ty else {
        return None;
    };
    if path.qself.is_some() {
        return None;
    }
    let segment = path.path.segments.last()?;
    if segment.ident != "Signal" {
        return None;
    }
    let PathArguments::AngleBracketed(arguments) = &segment.arguments else {
        return None;
    };
    arguments.args.iter().find_map(|argument| match argument {
        GenericArgument::Type(value) => Some(value.clone()),
        _ => None,
    })
}

/// 类型是否为 `Name<...>`（允许 `std::option::Option<U>` 等路径）。
fn is_named_generic(ty: &Type, name: &str) -> bool {
    let Type::Path(path) = ty else {
        return false;
    };
    if path.qself.is_some() {
        return false;
    }
    path.path.segments.last().is_some_and(|segment| {
        segment.ident == name && matches!(segment.arguments, syn::PathArguments::AngleBracketed(_))
    })
}
