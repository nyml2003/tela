//! `#[derive(DslComponent)]`：struct 字段即 Props，派生宏生成 Props 镜像与
//! `render` 脚手架（inject/provide/watch 能力 + 类型 assert）。

use proc_macro2::TokenStream as TokenStream2;
use quote::{format_ident, quote};
use syn::{Data, DeriveInput, Error, Expr, Field, Fields, Ident, Result, Type};

use crate::dsl_path;

/// `#[prop(...)]` / `#[inject]` / `#[provide]` / `#[watch(...)]` 字段语义。
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
    /// `#[provide]`：压入子作用域（T: Clone + Send + Sync + 'static）。
    Provide,
    /// `#[watch(key = "...")]`：订阅 Signal（类型必须 `Signal<T>`）。
    Watch,
}

struct FieldSpec {
    ident: Ident,
    ty: Type,
    kind: FieldKind,
    default_expr: Option<Expr>,
}

pub fn expand_derive(input: DeriveInput) -> Result<TokenStream2> {
    let name = input.ident.clone();
    // `#[memo]` 容器属性：opt-in 记忆化——指纹相等且子树订阅未标脏时跳过 render，
    // 直接拼回上次输出（见 tela-ui-dsl 的 `memo` 模块）。
    let memo = input.attrs.iter().any(|attr| attr.path().is_ident("memo"));
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

    // ---- 组装表达式 ----
    let assembly = specs.iter().map(|spec| {
        let ident = &spec.ident;
        match spec.kind {
            FieldKind::Plain => quote!(#ident: props.#ident.unwrap_or_default(),),
            FieldKind::Defaulted => {
                let default = spec.default_expr.as_ref().expect("defaulted has expr");
                quote!(#ident: props.#ident.unwrap_or(#default),)
            }
            FieldKind::Option => quote!(#ident: props.#ident,),
            FieldKind::Inject => {
                let binding = format_ident!("__tela_inject_{ident}");
                quote!(#ident: #binding,)
            }
            FieldKind::Provide => {
                let binding = format_ident!("__tela_provide_{ident}");
                quote!(#ident: #binding.clone(),)
            }
            FieldKind::Watch => {
                let binding = format_ident!("__tela_watch_{ident}");
                quote!(#ident: #binding.clone(),)
            }
        }
    });

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
    // ---- watch 绑定：缺失时整帧返回错误（不 panic）----
    let dsl = dsl_path();
    let watch_code = specs.iter().filter_map(|spec| {
        if spec.kind != FieldKind::Watch {
            return None;
        }
        let ident = &spec.ident;
        let binding = format_ident!("__tela_watch_{ident}");
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
    let watch_handles = specs
        .iter()
        .filter(|spec| spec.kind == FieldKind::Watch)
        .map(|spec| {
            let ident = &spec.ident;
            let binding = format_ident!("__tela_watch_{ident}");
            quote!(__tela_build.watch_source(&#binding, __tela_site))
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
            quote!(#dsl::ProvidedValue::new::<#ty>(#binding))
        });

    // ---- 类型 assert：仅定义检查函数并在未执行的 __check 中引用（编译期强制）----
    let assert_calls = specs.iter().map(|spec| {
        let ty = &spec.ty;
        match spec.kind {
            FieldKind::Plain => {
                quote!(__assert_default::<#ty>();)
            }
            FieldKind::Watch => quote!(),
            FieldKind::Inject | FieldKind::Provide => {
                quote!(__assert_clone::<#ty>();)
            }
            FieldKind::Defaulted | FieldKind::Option => quote!(),
        }
    });

    // ---- render 主体 ----
    let has_provide = specs.iter().any(|spec| spec.kind == FieldKind::Provide);
    let view_call = if has_provide {
        quote! {
            __tela_build.with_scope(
                vec![#(#provided_values),*],
                __tela_site,
                |__tela_build| {
                    let __tela_body = __tela_children.build(__tela_build)?;
                    __tela_inst.view(__tela_build, __tela_body)
                },
            )
        }
    } else {
        quote!({
            let __tela_body = __tela_children.build(__tela_build)?;
            __tela_inst.view(__tela_build, __tela_body)
        })
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

    // `#[memo]`：生成 props 指纹结构与 render 记忆化分支。
    let (memo_fp_impl, memo_render) = if memo {
        let fp_name = format_ident!("{}MemoFingerprint", name);
        let fp_fields = specs.iter().map(|spec| {
            let ident = &spec.ident;
            match spec.kind {
                FieldKind::Watch => quote!(#ident: #dsl::SignalId,),
                _ => {
                    let ty = &spec.ty;
                    quote!(#ident: #ty,)
                }
            }
        });
        let fp_build = specs.iter().map(|spec| {
            let ident = &spec.ident;
            match spec.kind {
                FieldKind::Watch => quote!(#ident: self.#ident.id(),),
                _ => quote!(#ident: self.#ident.clone(),),
            }
        });
        (
            quote! {
                const _: () = {
                    #[derive(Clone, PartialEq)]
                    struct #fp_name {
                        #(#fp_fields)*
                    }

                    impl #name {
                        #[doc(hidden)]
                        fn __tela_memo_fingerprint(&self) -> #fp_name {
                            #fp_name {
                                #(#fp_build)*
                            }
                        }
                    }
                };
            },
            quote! {
                let __tela_memo = __tela_build.memo_enabled() && __tela_children.is_empty();
                if __tela_memo {
                    let __tela_fingerprint = __tela_inst.__tela_memo_fingerprint();
                    if let Some(__tela_cached) = __tela_build.memo_hit(&__tela_fingerprint) {
                        // 命中：缓存输出已携带 watch 声明，直接拼回，跳过 view 与重复 attach。
                        return Ok(__tela_cached);
                    }
                    let mut __tela_out = #view_call?;
                    #watch_attach
                    // 记录必须发生在 watch attach 之后：缓存条目要携带组件自身的订阅，
                    // 否则脏检查看不到它的 scope，signal 变化会被误命中。
                    __tela_build.memo_record(__tela_fingerprint, &__tela_out);
                    Ok(__tela_out)
                } else {
                    let mut __tela_out = #view_call?;
                    #watch_attach
                    Ok(__tela_out)
                }
            },
        )
    } else {
        (
            quote!(),
            quote! {
                let mut __tela_out = #view_call?;
                #watch_attach
                Ok(__tela_out)
            },
        )
    };

    let impl_block = quote! {
        impl #dsl::DslComponent for #name {
            type Props = #props_name;
            type State = ();
            type Event = ();
            type Output = ();

            #identity_key

            fn render<'__tela_children, A>(
                __tela_context: &mut #dsl::ComponentRenderContext<'_, A>,
                props: Self::Props,
                _state: &Self::State,
                __tela_children: #dsl::Children<'__tela_children, A>,
            ) -> #dsl::ViewResult<#dsl::ViewOutput<A>> {
                let __tela_site = __tela_context.site();
                let __tela_build = __tela_context.build();
                #(#inject_code)*
                #(#watch_code)*
                #(#provide_code)*
                let __tela_inst = #name {
                    #(#assembly)*
                };
                #memo_render
            }
        }

        #memo_fp_impl

        #[doc(hidden)]
        const _: () = {
            fn __assert_default<T: Default>() {}
            fn __assert_clone<T: Clone>() {}
            fn __check() {
                #(#assert_calls)*
            }
        };
    };

    let mut output = TokenStream2::new();
    output.extend(props_struct);
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
    let mut default_expr = None;

    for attr in &field.attrs {
        if !attr.path().is_ident("prop") {
            if attr.path().is_ident("inject") {
                set_kind(&mut kind, FieldKind::Inject, attr, &ident)?;
                continue;
            }
            if attr.path().is_ident("provide") {
                set_kind(&mut kind, FieldKind::Provide, attr, &ident)?;
                continue;
            }
            if attr.path().is_ident("watch") {
                set_kind(&mut kind, FieldKind::Watch, attr, &ident)?;
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

    // 类型校验：option 字段必须是 Option<U>；watch 字段必须是 Signal<T>。
    if kind == FieldKind::Option && !is_option_type(&ty) {
        return Err(Error::new_spanned(
            &ty,
            "#[prop(option)] field must have type Option<U>",
        ));
    }
    if kind == FieldKind::Watch && !is_named_generic(&ty, "Signal") {
        return Err(Error::new_spanned(
            &ty,
            "#[watch] field must have type Signal<T>",
        ));
    }

    Ok(FieldSpec {
        ident,
        ty,
        kind,
        default_expr,
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
                "field '{}' has conflicting DSL attributes (inject/provide/watch/prop are exclusive)",
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
