//! Procedural macros for Tela's application-composition DSL.
//!
//! 033 定稿：DSL 只有一个概念——组件。宏只做属性搬运工（零校验、零白名单）：
//! 收集标签属性 → `Props` 字面量 → `render` 调用；保留的 `output={function_path}`
//! 属性把类型化组件 Output 静态映射为应用动作。控制流语法（Fragment/For/
//! VirtualList）保留宏级；`@provide/@inject/@watch` 指令已删除（迁移为
//! `#[derive(DslComponent)]` 字段属性）。

#![forbid(unsafe_code)]
#![warn(missing_docs)]

use proc_macro::TokenStream;
use proc_macro_crate::{FoundCrate, crate_name};
use proc_macro2::{Span, TokenStream as TokenStream2};
use quote::quote;
use syn::{
    Error, Expr, Ident, Result, Token, braced,
    parse::{Parse, ParseStream},
    parse_macro_input,
};

mod derive;

/// Expands Tela's explicit application-composition syntax.
///
/// The public spelling is `ui!(build { ... })`.
/// 派生 `DslComponent`：struct 字段即 Props，生成 Props 镜像与 render 脚手架。
#[proc_macro_derive(DslComponent, attributes(prop, inject, provide, watch))]
pub fn dsl_component(input: TokenStream) -> TokenStream {
    let input = parse_macro_input!(input as syn::DeriveInput);
    match derive::expand_derive(input) {
        Ok(tokens) => tokens.into(),
        Err(error) => error.into_compile_error().into(),
    }
}

/// Expands Tela's application-composition DSL.
///
/// 公开拼写 `ui!(build { ... })`。标签统一降级为 `DslComponent::render` 调用；
/// `Fragment`/`For`/`VirtualList`/`ActionTarget` 保留宏级（控制流/动作绑定语法）。
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

/// 为同一真实 parent body 内透明 `<For>` 声明分配固定 namespace。
#[derive(Default)]
struct CollectionScopes {
    next: u64,
}

impl CollectionScopes {
    fn allocate(&mut self, span: Span) -> Result<u32> {
        let scope = u32::try_from(self.next).map_err(|_| {
            Error::new(
                span,
                "too many <For> or <VirtualList> declarations in one DSL body",
            )
        })?;
        self.next += 1;
        Ok(scope)
    }
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
}

struct Element {
    name: Ident,
    attributes: Vec<Attribute>,
    children: Vec<Item>,
    repeat_binding: Option<Ident>,
    self_closing: bool,
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

    let tag_name = name.to_string();
    if self_closing {
        if matches!(tag_name.as_str(), "For" | "VirtualList") {
            return Err(Error::new(
                name.span(),
                format!("<{tag_name}> requires a {{|item| ...}} body"),
            ));
        }
        return Ok(Element {
            name,
            attributes,
            children: Vec::new(),
            repeat_binding: None,
            self_closing,
        });
    }

    if matches!(tag_name.as_str(), "For" | "VirtualList") {
        let closure;
        braced!(closure in input);
        closure.parse::<Token![|]>().map_err(|_| {
            Error::new(
                closure.span(),
                format!("<{tag_name}> body must begin with |item|"),
            )
        })?;
        let binding = closure.parse::<Ident>()?;
        closure.parse::<Token![|]>().map_err(|_| {
            Error::new(closure.span(), format!("<{tag_name}> body must use |item|"))
        })?;
        let children = parse_items(&closure, None)?;
        let closing = parse_closing_tag(input)?;
        if closing != tag_name {
            return Err(Error::new(
                name.span(),
                format!("expected </{tag_name}>, found </{closing}>"),
            ));
        }
        return Ok(Element {
            name,
            attributes,
            children,
            repeat_binding: Some(binding),
            self_closing,
        });
    }

    let children = parse_items(input, Some(&tag_name))?;
    Ok(Element {
        name,
        attributes,
        children,
        repeat_binding: None,
        self_closing,
    })
}

fn parse_attributes(input: ParseStream<'_>) -> Result<Vec<Attribute>> {
    let mut attributes = Vec::new();
    while !input.peek(Token![>]) && !input.peek(Token![/]) {
        let name = input.parse::<Ident>()?;
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
        attributes.push(Attribute { name, value });
    }
    Ok(attributes)
}

fn expand(input: UiInput) -> Result<TokenStream2> {
    let dsl = dsl_path();
    let mut scopes = CollectionScopes::default();
    let body = generate_body(&input.items, &input.build, &dsl, &mut scopes)?;
    let build = input.build;
    let site = site(&dsl);
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

fn site(dsl: &TokenStream2) -> TokenStream2 {
    quote!(#dsl::ViewSite::new(file!(), line!(), column!()))
}

fn generate_body(
    items: &[Item],
    build: &Ident,
    dsl: &TokenStream2,
    scopes: &mut CollectionScopes,
) -> Result<TokenStream2> {
    let mut children = Vec::new();
    for item in items {
        children.push(generate_child(item, build, dsl, scopes)?);
    }
    Ok(quote! {{
        let __tela_dsl_children = vec![#(#children?),*];
        Ok(#dsl::Body::new(__tela_dsl_children, Vec::new()))
    }})
}

fn generate_child(
    item: &Item,
    build: &Ident,
    dsl: &TokenStream2,
    scopes: &mut CollectionScopes,
) -> Result<TokenStream2> {
    match item {
        Item::Element(element) => generate_element(element, build, dsl, scopes),
        Item::Expr(expression) => Ok(quote! {{
            #dsl::into_view_child(#expression)
        }}),
    }
}

fn generate_element(
    element: &Element,
    build: &Ident,
    dsl: &TokenStream2,
    scopes: &mut CollectionScopes,
) -> Result<TokenStream2> {
    match element.name.to_string().as_str() {
        // 控制流/动作绑定语法（非 UI 元素，保留宏级）。
        "Fragment" => generate_fragment(element, build, dsl, scopes),
        "For" => generate_for(element, false, build, dsl, scopes),
        "VirtualList" => generate_for(element, true, build, dsl, scopes),
        // ActionTarget 的动作类型依赖调用点的 A（Props 无法脱离 A 构造），
        // 保留宏内建（033 C1 修订：ActionTarget 不组件化）。
        "ActionTarget" => generate_action_target(element, build, dsl, scopes),
        // 其余标签全部是组件：原样保留标识符，由编译器校验类型/契约。
        _ => generate_component(element, build, dsl, scopes),
    }
}

/// 组件解析：收集标签属性 → `Props` 字面量 → `render` 调用。
///
/// Props 从 `Default::default()` 起步、逐个覆盖提供的字段（字段均 `pub`）。
/// 类型位置使用 qualified path（稳定），避免关联类型字面量的实验特性与
/// 泛型推断歧义。
fn generate_component(
    element: &Element,
    build: &Ident,
    dsl: &TokenStream2,
    scopes: &mut CollectionScopes,
) -> Result<TokenStream2> {
    let tag = &element.name;
    if element.repeat_binding.is_some() {
        return Err(Error::new(
            element.name.span(),
            "component tags do not take a |item| body",
        ));
    }
    let mut child_scopes = CollectionScopes::default();
    let body = generate_body(&element.children, build, dsl, &mut child_scopes)?;
    let _ = scopes;
    let mut output_attributes = element
        .attributes
        .iter()
        .filter(|attribute| attribute.name == "output");
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
        .filter(|attribute| attribute.name != "output")
        .map(|attribute| {
            let name = &attribute.name;
            let value = &attribute.value;
            // 属性值统一 `Some((expr).into())`：Props 字段约定为 Option<T>，
            // 字符串字面量（&str）经 Into<String> 转换。
            quote!(__tela_dsl_props.#name = Some((#value).into());)
        });
    let render = if let Some(attribute) = output {
        let output = &attribute.value;
        quote! {
            {
                let __tela_dsl_output: fn(<#tag as #dsl::DslComponent>::Output) -> _ = #output;
                #dsl::render_component_with_output::<#tag, _>(
                    build,
                    __tela_dsl_props,
                    #dsl::Children::new(|#build| {
                        let _ = &mut *#build;
                        #body
                    }),
                    __tela_dsl_output,
                    #dsl::ViewSite::new(file!(), line!(), column!())
                )
            }
        }
    } else {
        quote! {
            #dsl::render_component::<#tag, _>(
                build,
                __tela_dsl_props,
                #dsl::Children::new(|#build| {
                    let _ = &mut *#build;
                    #body
                }),
                #dsl::ViewSite::new(file!(), line!(), column!())
            )
        }
    };
    Ok(quote! {{
        let mut __tela_dsl_props: <#tag as #dsl::DslComponent>::Props =
            Default::default();
        #(#assignments)*
        #dsl::into_view_child(
            #render?
        )
    }})
}

fn generate_fragment(
    element: &Element,
    build: &Ident,
    dsl: &TokenStream2,
    scopes: &mut CollectionScopes,
) -> Result<TokenStream2> {
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
    let body = generate_body(&element.children, build, dsl, scopes)?;
    let site = site(dsl);
    Ok(quote! {{
        let __tela_dsl_body = #body?;
        #build.fragment(__tela_dsl_body, #site)
    }})
}

/// ActionTarget 宏内建：动作类型 `A` 依赖调用点，Props 无法脱离 `A` 构造（033 C1 修订）。
fn generate_action_target(
    element: &Element,
    build: &Ident,
    dsl: &TokenStream2,
    scopes: &mut CollectionScopes,
) -> Result<TokenStream2> {
    if element.self_closing {
        return Err(Error::new(
            element.name.span(),
            "ActionTarget requires exactly one real child",
        ));
    }
    const ACTION_ATTRIBUTES: &[&str] = &["action", "on_input", "on_submit", "on_cancel"];
    validate_attributes(&element.attributes, ACTION_ATTRIBUTES)?;
    if element.attributes.is_empty() {
        return Err(Error::new(
            element.name.span(),
            "ActionTarget requires at least one action, on_input, on_submit, or on_cancel attribute",
        ));
    }
    let body = generate_body(&element.children, build, dsl, scopes)?;
    let mut registrations = Vec::new();
    for attribute in &element.attributes {
        let site = site(dsl);
        match attribute.name.to_string().as_str() {
            "action" => {
                let value = &attribute.value;
                registrations.push(quote! {
                    __tela_dsl_target = __tela_dsl_target.action_at(#value, #site);
                });
            }
            "on_cancel" => {
                let value = &attribute.value;
                registrations.push(quote! {
                    __tela_dsl_target = __tela_dsl_target.on_cancel_at(#value, #site);
                });
            }
            "on_input" | "on_submit" => {
                let value = &attribute.value;
                let method = if attribute.name == "on_input" {
                    "on_input_at"
                } else {
                    "on_submit_at"
                };
                let method = Ident::new(method, attribute.name.span());
                if is_with_context_call(value) {
                    registrations.push(quote! {
                        __tela_dsl_target = __tela_dsl_target.#method(#value, #site);
                    });
                } else if matches!(value, Expr::Path(_)) {
                    registrations.push(quote! {
                        __tela_dsl_target = {
                            let __tela_dsl_mapper: fn(String) -> _ = #value;
                            __tela_dsl_target.#method(
                                #dsl::TextActionMap::unary(__tela_dsl_mapper),
                                #site,
                            )
                        };
                    });
                } else {
                    return Err(Error::new_spanned(
                        value,
                        "on_input and on_submit require a function path or with_context(value, mapper)",
                    ));
                }
            }
            _ => unreachable!("attributes were validated"),
        }
    }
    let site = site(dsl);
    Ok(quote! {{
        let __tela_dsl_body = #body?;
        let mut __tela_dsl_target = #dsl::ActionTarget::new();
        #(#registrations)*
        let __tela_dsl_view_node = #build.action_target(
            __tela_dsl_body,
            __tela_dsl_target,
            #site,
        )?;
        Ok(#dsl::ViewChild::view_node(__tela_dsl_view_node))
    }})
}

fn generate_for(
    element: &Element,
    virtual_list: bool,
    build: &Ident,
    dsl: &TokenStream2,
    scopes: &mut CollectionScopes,
) -> Result<TokenStream2> {
    let Some(binding) = &element.repeat_binding else {
        return Err(Error::new(
            element.name.span(),
            "For and VirtualList require a {|item| ...} body",
        ));
    };
    let item_source_name = if virtual_list { "items" } else { "each" };
    let allowed = if virtual_list {
        vec![
            "items",
            "total_items",
            "key",
            "item_height",
            "item_spacing",
            "overscan",
            "first_item_index",
        ]
    } else {
        vec!["each", "key"]
    };
    validate_attributes(&element.attributes, &allowed)?;
    let source = required_attribute(&element.attributes, item_source_name, element.name.span())?;
    let key = required_attribute(&element.attributes, "key", element.name.span())?;
    let mut item_scopes = CollectionScopes::default();
    let item_body = generate_body(&element.children, build, dsl, &mut item_scopes)?;
    let item_site = site(dsl);
    let collection_scope = scopes.allocate(element.name.span())?;
    let loop_body = quote! {
        let __tela_dsl_item_key = &(#key);
        let __tela_dsl_item_body = #build.with_item_identity(
            #collection_scope,
            __tela_dsl_item_key,
            |#build| #item_body,
        )?;
        let __tela_dsl_item = #build.for_item(
            __tela_dsl_item_body,
            __tela_dsl_item_key,
            #item_site,
        )?;
        __tela_dsl_items.push(__tela_dsl_item);
    };

    if !virtual_list {
        return Ok(quote! {{
            let mut __tela_dsl_items = Vec::new();
            for #binding in #source {
                #loop_body
            }
            Ok(#dsl::ViewChild::collection(#collection_scope, __tela_dsl_items))
        }});
    }

    let total_items = required_attribute(&element.attributes, "total_items", element.name.span())?;
    let item_height = required_attribute(&element.attributes, "item_height", element.name.span())?;
    let item_spacing =
        required_attribute(&element.attributes, "item_spacing", element.name.span())?;
    let overscan = required_attribute(&element.attributes, "overscan", element.name.span())?;
    let first_item_index =
        required_attribute(&element.attributes, "first_item_index", element.name.span())?;
    Ok(quote! {{
        let mut __tela_dsl_items = Vec::new();
        for #binding in #source {
            #loop_body
        }
        let __tela_dsl_body = #dsl::Body::new(
            vec![#dsl::ViewChild::collection(#collection_scope, __tela_dsl_items)],
            Vec::new(),
        );
        let __tela_dsl_node = #dsl::__private::UiNode::new(
            #dsl::__private::NodeKind::VirtualListView(#dsl::__private::VirtualListSpec {
                total_items: #total_items,
                first_item_index: #first_item_index,
                item_height: #item_height,
                item_spacing: #item_spacing,
                overscan: #overscan,
            }),
        );
        let __tela_dsl_view_node = #build.container(__tela_dsl_node, __tela_dsl_body)?;
        Ok(#dsl::ViewChild::view_node(__tela_dsl_view_node))
    }})
}

/// 表达式是否为 `with_context(value, mapper)` 调用。
fn is_with_context_call(expression: &Expr) -> bool {
    let Expr::Call(call) = expression else {
        return false;
    };
    matches!(
        &*call.func,
        Expr::Path(path)
            if path
                .path
                .segments
                .last()
                .is_some_and(|segment| segment.ident == "with_context")
    )
}

fn attribute<'a>(attributes: &'a [Attribute], wanted: &str) -> Option<&'a Attribute> {
    attributes.iter().find(|attribute| attribute.name == wanted)
}

fn required_attribute<'a>(
    attributes: &'a [Attribute],
    wanted: &str,
    span: Span,
) -> Result<&'a Expr> {
    attribute(attributes, wanted)
        .map(|attribute| &attribute.value)
        .ok_or_else(|| Error::new(span, format!("missing required {wanted}={{...}} attribute")))
}

fn validate_attributes(attributes: &[Attribute], allowed: &[&str]) -> Result<()> {
    let mut seen = std::collections::BTreeSet::new();
    for attribute in attributes {
        let name = attribute.name.to_string();
        if !allowed.contains(&name.as_str()) {
            return Err(Error::new(
                attribute.name.span(),
                format!("attribute {name} is not allowed on this DSL tag"),
            ));
        }
        if !seen.insert(name.clone()) {
            return Err(Error::new(
                attribute.name.span(),
                format!("duplicate attribute {name}"),
            ));
        }
    }
    Ok(())
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
    fn component_tags_lower_to_render_calls() {
        let input = parse("build { <NavButton label={\"设置\"} width={72.0}>{\"x\"}</NavButton> }");
        let tokens = expand(input).expect("component lowering must succeed");
        let code = tokens.to_string();
        assert!(code.contains("DslComponent"));
        assert!(code.contains("render"));
        assert!(code.contains("label"));
        assert!(code.contains("width"));
        assert!(code.contains("into"));
    }

    #[test]
    fn component_output_lowers_to_the_static_lifecycle_binding() {
        let input = parse("build { <Transfer items={items} output={map_transfer} /> }");
        let tokens = expand(input).expect("component output lowering must succeed");
        let code = tokens.to_string();
        assert!(code.contains("render_component_with_output"));
        assert!(code.contains("map_transfer"));
        assert!(!code.contains("props . output"));
    }
}
