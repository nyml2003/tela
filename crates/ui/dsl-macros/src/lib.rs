//! Procedural macros for Tela's application-composition DSL.
//!
//! DSL 只负责把标签、普通属性和显式 `@output` 连接搬运到组件的统一装配入口。
//! 它不按标签名、节点类型或叶子性推断能力；`For`、`Show` 等结构也是普通注册组件。

#![forbid(unsafe_code)]
#![warn(missing_docs)]

use proc_macro::TokenStream;
use proc_macro_crate::{FoundCrate, crate_name};
use proc_macro2::{Span, TokenStream as TokenStream2};
use quote::{quote, quote_spanned};
use syn::{
    Error, Expr, ExprLit, Ident, Lit, LitStr, Result, Token, braced,
    parse::{Parse, ParseStream},
    parse_macro_input,
};

mod derive;

/// Expands Tela's explicit application-composition syntax.
///
/// The public spelling is `ui!(build { ... })`.
/// 派生 `DslComponent`：struct 字段即 Props，生成 Props 镜像与 assemble 脚手架。
/// `#[bind(paint = function_path)]` 和 `#[bind(layout = function_path)]` 声明从一个
/// `Signal<T>` 到该组件输出根呈现字段的静态边；它们生成现有 `StaticBindingTable` 的
/// 样板，不扫描 view 函数或自动订阅任意读取。
#[proc_macro_derive(DslComponent, attributes(prop, inject, provide, watch, bind))]
pub fn dsl_component(input: TokenStream) -> TokenStream {
    let input = parse_macro_input!(input as syn::DeriveInput);
    match derive::expand_derive(input) {
        Ok(tokens) => tokens.into(),
        Err(error) => error.into_compile_error().into(),
    }
}

/// Expands Tela's application-composition DSL.
///
/// 公开拼写 `ui!(build { ... })`。标签统一装配为 `UiSpec::assemble` 调用。
#[proc_macro]
pub fn ui(input: TokenStream) -> TokenStream {
    let input = parse_macro_input!(input as UiInput);
    match expand(input) {
        Ok(tokens) => tokens.into(),
        Err(error) => error.into_compile_error().into(),
    }
}

struct UiInput {
    build: Ident,
    items: Vec<Item>,
}

impl Parse for UiInput {
    fn parse(input: ParseStream<'_>) -> Result<Self> {
        let build = input.parse::<Ident>().map_err(|_| {
            Error::new(
                input.span(),
                "ui! expects an explicit ViewBuild identifier: ui!(build { ... })",
            )
        })?;
        if input.peek(Token![,]) {
            return Err(Error::new(
                input.span(),
                "ui! uses ui!(build { ... }); the comma form is no longer supported",
            ));
        }
        let body;
        braced!(body in input);
        if !input.is_empty() {
            return Err(input.error("unexpected tokens after ui! body"));
        }
        Ok(Self {
            build,
            items: parse_items(&body, None)?,
        })
    }
}

enum Item {
    Element(Element),
    Expr(Expr),
}

struct Attribute {
    name: Ident,
    value: Expr,
    output: bool,
}

struct Element {
    name: Ident,
    attributes: Vec<Attribute>,
    children: Vec<Item>,
    self_closing: bool,
}

/// A direct `<Fragment slot={"name"}>...</Fragment>` child of an arbitrary component.
///
/// This is generic `ui!` structure syntax, not a component-name hook: the receiving component
/// decides whether to consume the slot through `Children::build_named`. The literal name is part
/// of that component invocation's static template contract.
struct NamedSlotFragment<'a> {
    name: LitStr,
    children: &'a [Item],
}

fn parse_items(input: ParseStream<'_>, closing: Option<&str>) -> Result<Vec<Item>> {
    let mut items = Vec::new();

    while !input.is_empty() {
        if input.peek(Token![<]) && is_closing_tag(input)? {
            let closing_name = parse_closing_tag(input)?;
            let Some(expected) = closing else {
                return Err(Error::new(Span::call_site(), "unexpected closing DSL tag"));
            };
            if closing_name != expected {
                return Err(Error::new(
                    Span::call_site(),
                    format!("expected </{expected}>, found </{closing_name}>"),
                ));
            }
            return Ok(items);
        }

        if input.peek(Token![@]) {
            return Err(Error::new(
                Span::call_site(),
                "@provide/@inject/@watch directives are removed; use #[derive(DslComponent)] field attributes instead (see docs/033)",
            ));
        }

        if input.peek(Token![<]) {
            items.push(Item::Element(parse_element(input)?));
        } else if input.peek(syn::token::Brace) {
            let content;
            braced!(content in input);
            let expression = content.parse::<Expr>()?;
            if !content.is_empty() {
                return Err(
                    content.error("a DSL expression child must contain one Rust expression")
                );
            }
            items.push(Item::Expr(expression));
        } else {
            return Err(input.error("expected <Tag>, or { Rust expression }"));
        }
    }

    if let Some(expected) = closing {
        return Err(Error::new(
            input.span(),
            format!("missing closing </{expected}>"),
        ));
    }
    Ok(items)
}

fn is_closing_tag(input: ParseStream<'_>) -> Result<bool> {
    let fork = input.fork();
    fork.parse::<Token![<]>()?;
    Ok(fork.peek(Token![/]))
}

fn parse_closing_tag(input: ParseStream<'_>) -> Result<String> {
    input.parse::<Token![<]>()?;
    input.parse::<Token![/]>()?;
    let name = input.parse::<Ident>()?;
    input.parse::<Token![>]>()?;
    Ok(name.to_string())
}

fn parse_element(input: ParseStream<'_>) -> Result<Element> {
    input.parse::<Token![<]>()?;
    let name = input.parse::<Ident>()?;
    let attributes = parse_attributes(input)?;
    let self_closing = if input.peek(Token![/]) {
        input.parse::<Token![/]>()?;
        input.parse::<Token![>]>()?;
        true
    } else {
        input.parse::<Token![>]>()?;
        false
    };

    if self_closing {
        return Ok(Element {
            name,
            attributes,
            children: Vec::new(),
            self_closing,
        });
    }

    let tag_name = name.to_string();
    let children = parse_items(input, Some(&tag_name))?;
    Ok(Element {
        name,
        attributes,
        children,
        self_closing,
    })
}

fn parse_attributes(input: ParseStream<'_>) -> Result<Vec<Attribute>> {
    let mut attributes = Vec::new();
    while !input.peek(Token![>]) && !input.peek(Token![/]) {
        let output = if input.peek(Token![@]) {
            input.parse::<Token![@]>()?;
            true
        } else {
            false
        };
        let name = input.parse::<Ident>()?;
        if output && name != "output" {
            return Err(Error::new(
                name.span(),
                "the only @ attribute is @output={function_path}",
            ));
        }
        input.parse::<Token![=]>().map_err(|_| {
            Error::new(
                name.span(),
                "DSL attributes require a braced Rust expression, for example gap={12.0}",
            )
        })?;
        let content;
        braced!(content in input);
        let value = content.parse::<Expr>()?;
        if !content.is_empty() {
            return Err(content.error("a DSL attribute must contain one Rust expression"));
        }
        attributes.push(Attribute {
            name,
            value,
            output,
        });
    }
    Ok(attributes)
}

fn expand(input: UiInput) -> Result<TokenStream2> {
    let dsl = dsl_path();
    let body = generate_body(&input.items, &input.build, &dsl)?;
    let build = input.build;
    let site = site(&dsl, Span::call_site());
    Ok(quote! {{
        let __tela_dsl_body = #body?;
        #build.finish(__tela_dsl_body, #site)
    }})
}

pub(crate) fn dsl_path() -> TokenStream2 {
    match crate_name("tela-ui-dsl") {
        Ok(FoundCrate::Itself) => quote!(::tela_ui_dsl),
        Ok(FoundCrate::Name(name)) => {
            let ident = Ident::new(&name, Span::call_site());
            quote!(::#ident)
        }
        Err(_) => quote!(::tela_ui_dsl),
    }
}

fn site(dsl: &TokenStream2, span: Span) -> TokenStream2 {
    // The built-in location macros inherit this span, so every tag gets its own stable lexical
    // coordinate even when several tags live inside one `ui!` invocation.  Identity must never
    // fall back to the outer macro call site: same-type sibling components would otherwise share
    // State and a transparent `For` namespace.
    quote_spanned!(span=> #dsl::ViewSite::new(file!(), line!(), column!()))
}

fn generate_body(items: &[Item], build: &Ident, dsl: &TokenStream2) -> Result<TokenStream2> {
    let items = items.iter().collect::<Vec<_>>();
    generate_body_refs(&items, build, dsl)
}

fn generate_body_refs(items: &[&Item], build: &Ident, dsl: &TokenStream2) -> Result<TokenStream2> {
    let mut children = Vec::new();
    for item in items {
        children.push(generate_child(item, build, dsl)?);
    }
    Ok(quote! {{
        let __tela_dsl_children = vec![#(#children?),*];
        Ok(#dsl::Body::new(__tela_dsl_children, Vec::new()))
    }})
}

fn generate_child(item: &Item, build: &Ident, dsl: &TokenStream2) -> Result<TokenStream2> {
    match item {
        Item::Element(element) => generate_element(element, build, dsl),
        Item::Expr(expression) => Ok(quote! {{
            #dsl::into_view_child(#expression)
        }}),
    }
}

fn generate_element(element: &Element, build: &Ident, dsl: &TokenStream2) -> Result<TokenStream2> {
    match element.name.to_string().as_str() {
        // `Fragment` is the only syntax-level transparent grouping form. `For`, `Show` and
        // every interactive surface assembles through the ordinary component path below.
        "Fragment" => generate_fragment(element, build, dsl),
        // Every other tag is a component. The macro never treats leaves, collection names, or
        // application actions as privileged syntax.
        _ => generate_component(element, build, dsl),
    }
}

/// 组件解析：收集标签属性 → `Props` 字面量 → `assemble` 调用。
///
/// Props 从 `Default::default()` 起步、逐个覆盖提供的字段（字段均 `pub`）。
/// 类型位置使用 qualified path（稳定），避免关联类型字面量的实验特性与
/// 泛型推断歧义。
fn generate_component(
    element: &Element,
    build: &Ident,
    dsl: &TokenStream2,
) -> Result<TokenStream2> {
    let tag = &element.name;
    let component_site = site(dsl, tag.span());
    let (default_children, named_children) = partition_component_children(&element.children)?;
    let body = generate_body_refs(&default_children, build, dsl)?;
    let mut output_attributes = element
        .attributes
        .iter()
        .filter(|attribute| attribute.output);
    let output = output_attributes.next();
    if let Some(duplicate) = output_attributes.next() {
        return Err(Error::new(
            duplicate.name.span(),
            "duplicate attribute output",
        ));
    }
    if let Some(attribute) = output
        && !matches!(attribute.value, Expr::Path(_))
    {
        return Err(Error::new_spanned(
            &attribute.value,
            "component output requires a function path",
        ));
    }
    let assignments = element
        .attributes
        .iter()
        .filter(|attribute| !attribute.output)
        .map(|attribute| {
            let name = &attribute.name;
            let value = &attribute.value;
            // 属性统一交给 Props 字段的槽位解释：普通 `Option<T>` 走 `Into<T>`，带有
            // 精确函数签名的结构组件槽位则自行限制输入。宏不按组件类型分支。
            quote!(__tela_dsl_props.#name.assign(#value);)
        });
    // Named fragments are split before the component is assembled. Every slot remains a
    // FnOnce until its receiving component explicitly consumes it; the macro never traverses a
    // completed child tree or infers which component capability a slot represents.
    let named_slot_builders = named_children
        .iter()
        .map(|slot| {
            let name = &slot.name;
            let body = generate_body(slot.children, build, dsl)?;
            Ok(quote! {
                #dsl::NamedSlot::new(#dsl::SlotName::new(#name), |#build| {
                    let _ = &mut *#build;
                    #body
                })
            })
        })
        .collect::<Result<Vec<_>>>()?;
    // No named slot keeps the old compact construction path. Otherwise a default closure exists
    // only when there are direct, unlabelled children; this preserves `Children::is_empty()` for
    // structural components that reject all child slots.
    let children_expr = if named_slot_builders.is_empty() {
        if default_children.is_empty() {
            quote!(#dsl::Children::empty())
        } else {
            quote!(#dsl::Children::new(|#build| {
                let _ = &mut *#build;
                #body
            }))
        }
    } else if default_children.is_empty() {
        quote!(#dsl::Children::with_named_slots(vec![#(#named_slot_builders),*])?)
    } else {
        quote!(#dsl::Children::with_default_and_named_slots(
            |#build| {
                let _ = &mut *#build;
                #body
            },
            vec![#(#named_slot_builders),*],
        )?)
    };
    let assembly = if let Some(attribute) = output {
        let output = &attribute.value;
        quote! {
            {
                let __tela_dsl_output = #dsl::component_output_mapper::<#tag, _, _>(
                    #build,
                    #output,
                );
                #dsl::assemble_component_with_output::<#tag, _, _>(
                    #build,
                    __tela_dsl_props,
                    #children_expr,
                    __tela_dsl_output,
                    stringify!(#output),
                    #component_site
                )
            }
        }
    } else {
        quote! {
            #dsl::assemble_component::<#tag, _>(
                #build,
                __tela_dsl_props,
                #children_expr,
                #component_site
            )
        }
    };
    Ok(quote! {{
        use #dsl::DslPropSlot as _;
        let mut __tela_dsl_props = #dsl::default_component_props::<#tag, _>(#build);
        #(#assignments)*
        #dsl::into_view_child(
            #assembly?
        )
    }})
}

fn partition_component_children<'a>(
    children: &'a [Item],
) -> Result<(Vec<&'a Item>, Vec<NamedSlotFragment<'a>>)> {
    let mut default = Vec::new();
    let mut named = Vec::new();
    let mut names = std::collections::BTreeSet::new();

    for child in children {
        let Item::Element(element) = child else {
            default.push(child);
            continue;
        };
        if element.name != "Fragment" {
            default.push(child);
            continue;
        }
        let slot_attributes = element
            .attributes
            .iter()
            .filter(|attribute| !attribute.output && attribute.name == "slot")
            .collect::<Vec<_>>();
        if slot_attributes.is_empty() {
            default.push(child);
            continue;
        }
        let slot = slot_attributes[0];
        if element.self_closing {
            return Err(Error::new(
                slot.name.span(),
                "a named <Fragment slot={\"...\"}> requires a closing tag",
            ));
        }
        if element.attributes.len() != 1 || slot_attributes.len() != 1 {
            return Err(Error::new(
                slot.name.span(),
                "a named Fragment accepts only slot={\"static-name\"}",
            ));
        }
        let Expr::Lit(ExprLit {
            lit: Lit::Str(name),
            ..
        }) = &slot.value
        else {
            return Err(Error::new_spanned(
                &slot.value,
                "a named Fragment slot must be a string literal, for example slot={\"header\"}",
            ));
        };
        if name.value().is_empty() {
            return Err(Error::new(
                name.span(),
                "a named Fragment slot cannot be empty",
            ));
        }
        if !names.insert(name.value()) {
            return Err(Error::new(
                name.span(),
                "duplicate named Fragment slot in one component invocation",
            ));
        }
        named.push(NamedSlotFragment {
            name: name.clone(),
            children: &element.children,
        });
    }

    Ok((default, named))
}

fn generate_fragment(element: &Element, build: &Ident, dsl: &TokenStream2) -> Result<TokenStream2> {
    if element.self_closing {
        return Err(Error::new(
            element.name.span(),
            "<Fragment> requires children and cannot be a top-level root",
        ));
    }
    if !element.attributes.is_empty() {
        return Err(Error::new(
            element.attributes[0].name.span(),
            "Fragment is identity-transparent and accepts no attributes",
        ));
    }
    let body = generate_body(&element.children, build, dsl)?;
    let site = site(dsl, element.name.span());
    Ok(quote! {{
        let __tela_dsl_body = #body?;
        #build.fragment(__tela_dsl_body, #site)
    }})
}

#[cfg(test)]
mod tests {
    use super::{UiInput, expand};

    fn parse(source: &str) -> UiInput {
        syn::parse_str(source).expect("test DSL input must parse")
    }

    #[test]
    fn accepts_the_block_style_public_invocation() {
        let input = parse("build { <Text value={\"ready\"} /> }");
        assert_eq!(input.items.len(), 1);
    }

    #[test]
    fn rejects_the_removed_comma_invocation() {
        let error = match syn::parse_str::<UiInput>("build, { <Text value={\"ready\"} /> }") {
            Ok(_) => panic!("the comma form must not parse"),
            Err(error) => error,
        };

        assert!(
            error
                .to_string()
                .contains("ui!(build { ... }); the comma form is no longer supported")
        );
    }

    #[test]
    fn rejects_removed_directives_with_migration_hint() {
        let error = match syn::parse_str::<UiInput>(
            "build { @watch(count, &state.count); <Text value={\"x\"} /> }",
        ) {
            Ok(_) => panic!("directives must be rejected"),
            Err(error) => error,
        };
        assert!(error.to_string().contains("derive(DslComponent)"));
    }

    #[test]
    fn component_tags_assemble_to_spec_calls() {
        let input = parse("build { <NavButton label={\"设置\"} width={72.0}>{\"x\"}</NavButton> }");
        let tokens = expand(input).expect("component assembly must succeed");
        let code = tokens.to_string();
        assert!(code.contains("assemble_component"));
        assert!(code.contains("label"));
        assert!(code.contains("width"));
        assert!(code.contains("assign"));
        assert!(code.contains("ViewSite"));
    }

    #[test]
    fn component_output_assembles_to_the_static_lifecycle_binding() {
        let input = parse("build { <Transfer items={items} @output={map_transfer} /> }");
        let tokens = expand(input).expect("component output assembly must succeed");
        let code = tokens.to_string();
        assert!(code.contains("assemble_component_with_output"));
        assert!(code.contains("map_transfer"));
        assert!(!code.contains("props . output"));
    }
}
