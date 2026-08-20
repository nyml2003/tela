//! Procedural macros for Tela's application-composition DSL.

#![forbid(unsafe_code)]
#![warn(missing_docs)]

use proc_macro::TokenStream;
use proc_macro_crate::{FoundCrate, crate_name};
use proc_macro2::{Span, TokenStream as TokenStream2};
use quote::{format_ident, quote};
use syn::{
    Error, Expr, ExprCall, ExprClosure, ExprPath, Ident, Result, Token, Type, braced,
    parenthesized,
    parse::{Parse, ParseStream},
    parse_macro_input,
    spanned::Spanned,
    visit::{self, Visit},
};

/// Expands Tela's explicit application-composition syntax.
///
/// The public spelling is `ui!(build { ... })`. The `build` identifier is deliberately explicit
/// and is shadowed only inside generated lexical scope closures.
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
///
/// 这个计数器只在 macro 展开期按词法声明顺序推进，绝不能根据本帧实际 item 数或
/// `ViewChild` flatten 结果生成。真实结构节点和 item body 各自拥有新的 allocator；
/// Fragment 与 ActionTarget 因为透明而复用当前 allocator。
#[derive(Default)]
struct CollectionScopes {
    // Keep a sentinel value above `u32::MAX` so the final representable scope can still be
    // allocated once. Saturating a `u32` here would silently reuse the final namespace.
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
    Provide(Provide),
    Inject(Inject),
    Watch(Watch),
    Element(Element),
    Expr(Expr),
}

struct Provide {
    value: Expr,
    ty: Type,
    span: Span,
}

struct Inject {
    name: Ident,
    ty: Type,
    span: Span,
}

struct Watch {
    name: Ident,
    source: Expr,
    span: Span,
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
    let mut phase = DirectivePhase::Provide;

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
            let directive = parse_directive(input)?;
            match &directive {
                Item::Provide(_) if phase == DirectivePhase::Provide => {}
                Item::Provide(_) => {
                    return Err(Error::new(
                        directive_span(&directive),
                        "@provide must appear before @inject, @watch, tags, and child expressions",
                    ));
                }
                Item::Inject(_) | Item::Watch(_) if phase != DirectivePhase::Body => {
                    phase = DirectivePhase::Dependencies;
                }
                Item::Inject(_) | Item::Watch(_) => {
                    return Err(Error::new(
                        directive_span(&directive),
                        "@inject and @watch must appear before tags and child expressions",
                    ));
                }
                _ => unreachable!("parse_directive only returns directives"),
            }
            items.push(directive);
            continue;
        }

        phase = DirectivePhase::Body;
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
            return Err(input.error("expected @directive, <Tag>, or { Rust expression }"));
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

#[derive(Clone, Copy, Eq, PartialEq)]
enum DirectivePhase {
    Provide,
    Dependencies,
    Body,
}

/// `ui!` itself creates an explicit lexical capability scope. Real node bodies intentionally do
/// not: otherwise an incidental visual wrapper would silently become a Context boundary.
#[derive(Clone, Copy, Eq, PartialEq)]
enum BodyScope {
    ExplicitUi,
    Node,
}

fn directive_span(item: &Item) -> Span {
    match item {
        Item::Provide(provide) => provide.span,
        Item::Inject(inject) => inject.span,
        Item::Watch(watch) => watch.span,
        Item::Element(element) => element.name.span(),
        Item::Expr(expression) => expression.span(),
    }
}

fn parse_directive(input: ParseStream<'_>) -> Result<Item> {
    let at = input.parse::<Token![@]>()?;
    let name = input.parse::<Ident>()?;
    let directive = name.to_string();
    if directive == "batch" {
        return Err(Error::new(
            name.span(),
            "@batch is not a DSL directive; call runtime.batch(|| { ... }) in ActionHandler",
        ));
    }
    if !matches!(directive.as_str(), "provide" | "inject" | "watch") {
        return Err(Error::new(
            name.span(),
            "unknown DSL directive; supported directives are @provide, @inject, and @watch",
        ));
    }
    let content;
    parenthesized!(content in input);
    let item = match directive.as_str() {
        "provide" => {
            let value = content.parse::<Expr>()?;
            content.parse::<Token![:]>().map_err(|_| {
                Error::new(
                    content.span(),
                    "@provide requires an explicit type: @provide(value: Type);",
                )
            })?;
            let ty = content.parse::<Type>()?;
            if !content.is_empty() {
                return Err(content.error("unexpected tokens in @provide"));
            }
            Item::Provide(Provide {
                value,
                ty,
                span: at.span,
            })
        }
        "inject" => {
            let local = content.parse::<Ident>()?;
            content.parse::<Token![:]>().map_err(|_| {
                Error::new(
                    content.span(),
                    "@inject requires an explicit type: @inject(name: Type);",
                )
            })?;
            let ty = content.parse::<Type>()?;
            if !content.is_empty() {
                return Err(content.error("unexpected tokens in @inject"));
            }
            Item::Inject(Inject {
                name: local,
                ty,
                span: at.span,
            })
        }
        "watch" => {
            let local = content.parse::<Ident>()?;
            content.parse::<Token![,]>().map_err(|_| {
                Error::new(content.span(), "@watch uses @watch(local_name, &signal);")
            })?;
            let source = content.parse::<Expr>()?;
            if !matches!(source, Expr::Reference(_)) {
                return Err(Error::new(
                    source.span(),
                    "@watch requires an explicit Signal reference: @watch(name, &signal);",
                ));
            }
            if !content.is_empty() {
                return Err(content.error("unexpected tokens in @watch"));
            }
            Item::Watch(Watch {
                name: local,
                source,
                span: at.span,
            })
        }
        _ => unreachable!("directive was checked before parsing its arguments"),
    };
    input
        .parse::<Token![;]>()
        .map_err(|_| Error::new(name.span(), "DSL directives end with a semicolon"))?;
    Ok(item)
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
    let body = generate_body(
        &input.items,
        &input.build,
        &dsl,
        &mut scopes,
        BodyScope::ExplicitUi,
    )?;
    let build = input.build;
    let site = site(&dsl);
    Ok(quote! {{
        let __tela_dsl_body = #body?;
        #build.finish(__tela_dsl_body, #site)
    }})
}

fn dsl_path() -> TokenStream2 {
    match crate_name("tela-ui-dsl") {
        // `proc_macro_crate` also reports `Itself` for examples belonging to the same package.
        // In that context `crate` denotes the example binary, not the library that re-exports
        // the DSL runtime. `tela-ui-dsl` declares a self alias for library-internal expansions.
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
    body_scope: BodyScope,
) -> Result<TokenStream2> {
    let mut providers = Vec::new();
    let mut injects = Vec::new();
    let mut watches = Vec::new();
    let mut children = Vec::new();

    for item in items {
        match item {
            Item::Provide(provide) => {
                if body_scope == BodyScope::Node {
                    return Err(Error::new(
                        provide.span,
                        "@provide is only valid in an explicit ui!(build { ... }) block; a real node body does not create a capability scope",
                    ));
                }
                providers.push(provide);
            }
            Item::Inject(inject) => {
                if body_scope == BodyScope::Node {
                    return Err(Error::new(
                        inject.span,
                        "@inject is only valid in an explicit ui!(build { ... }) block; use a nested ui! expression to create a capability scope",
                    ));
                }
                injects.push(inject);
            }
            Item::Watch(watch) => watches.push(watch),
            Item::Element(_) | Item::Expr(_) => {
                children.push(generate_child(item, build, dsl, scopes)?)
            }
        }
    }

    let provider_values = providers.iter().map(|provider| {
        let value = &provider.value;
        let ty = &provider.ty;
        quote!(#dsl::ProvidedValue::new::<#ty>(#value))
    });
    let injection_code = injects.iter().map(|inject| {
        let name = &inject.name;
        let ty = &inject.ty;
        let site = site(dsl);
        quote!(let #name = __tela_dsl_scope.inject::<#ty>(#site)?;)
    });
    let watch_code = watches.iter().enumerate().map(|(index, watch)| {
        let name = &watch.name;
        let source = &watch.source;
        let handle = format_ident!("__tela_dsl_watch_{index}");
        let site = site(dsl);
        quote! {
            let #name = #dsl::Signal::clone(#source);
            let #handle = #build.watch_source(&#name, #site);
        }
    });
    let watch_handles = (0..watches.len()).map(|index| format_ident!("__tela_dsl_watch_{index}"));
    let scope_site = site(dsl);

    let body = quote! {
        #(#watch_code)*
        let __tela_dsl_children = vec![#(#children?),*];
        Ok(#dsl::Body::new(
            __tela_dsl_children,
            vec![#(#watch_handles),*],
        ))
    };

    match body_scope {
        BodyScope::ExplicitUi => Ok(quote! {
            #build.with_scope(
                vec![#(#provider_values),*],
                #scope_site,
                |#build| {
                    let __tela_dsl_scope = #build.current_scope();
                    #(#injection_code)*
                    #body
                },
            )
        }),
        BodyScope::Node => Ok(quote! {{ #body }}),
    }
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
        Item::Provide(_) | Item::Inject(_) | Item::Watch(_) => Err(Error::new(
            directive_span(item),
            "directives are only valid in a DSL body, not as a child node",
        )),
    }
}

fn generate_element(
    element: &Element,
    build: &Ident,
    dsl: &TokenStream2,
    scopes: &mut CollectionScopes,
) -> Result<TokenStream2> {
    match element.name.to_string().as_str() {
        "Column" => generate_container(element, "Column", build, dsl),
        "Row" => generate_container(element, "Row", build, dsl),
        "Frame" => generate_container(element, "Frame", build, dsl),
        "View" => generate_container(element, "View", build, dsl),
        "Stack" => generate_container(element, "Stack", build, dsl),
        "ScrollView" => generate_container(element, "ScrollView", build, dsl),
        "Fragment" => generate_fragment(element, build, dsl, scopes),
        "ActionTarget" => generate_action_target(element, build, dsl, scopes),
        "For" => generate_for(element, false, build, dsl, scopes),
        "VirtualList" => generate_for(element, true, build, dsl, scopes),
        "Text" => generate_text(element, false, dsl),
        "Icon" => generate_text(element, true, dsl),
        "Image" => generate_image(element, dsl),
        _ => Err(Error::new(
            element.name.span(),
            "unsupported DSL tag; V1 supports Column, Row, Frame, View, Stack, ScrollView, Text, Icon, Image, For, VirtualList, Fragment, and ActionTarget",
        )),
    }
}

const COMMON_ATTRIBUTES: &[&str] = &[
    "key",
    "width",
    "height",
    "margin",
    "padding",
    "border_width",
    "gap",
    "cross_align",
    "clip",
    "overflow",
    "grid_item",
    "text_constraint",
    "fill",
    "border_color",
    "border_radius",
    "shadow",
    "draw_order",
    "visual_offset",
    "clickable",
    "hoverable",
    "focusable",
    "tab_index",
    "input",
    "bind_id",
    "pointer_capture",
    "gestures",
    "modal",
];

fn generate_container(
    element: &Element,
    kind: &str,
    build: &Ident,
    dsl: &TokenStream2,
) -> Result<TokenStream2> {
    if element.self_closing && kind == "Frame" {
        return Err(Error::new(
            element.name.span(),
            "<Frame> requires one real child",
        ));
    }
    validate_attributes(&element.attributes, COMMON_ATTRIBUTES)?;
    let mut child_scopes = CollectionScopes::default();
    let body = generate_body(
        &element.children,
        build,
        dsl,
        &mut child_scopes,
        BodyScope::Node,
    )?;
    let attrs = generate_node_attributes(&element.attributes, dsl)?;
    let node_kind = match kind {
        "Column" => quote!(#dsl::__private::NodeKind::Column),
        "Row" => quote!(#dsl::__private::NodeKind::Row),
        "Frame" => quote!(#dsl::__private::NodeKind::Frame),
        "View" => quote!(#dsl::__private::NodeKind::View),
        "Stack" => quote!(#dsl::__private::NodeKind::Stack),
        "ScrollView" => quote!(#dsl::__private::NodeKind::ScrollView),
        _ => unreachable!("container kind is checked by caller"),
    };
    let key = attribute(&element.attributes, "key").map(|attribute| &attribute.value);
    let keyed = key.map_or_else(
        || quote!(__tela_dsl_view_node),
        |key| quote!(__tela_dsl_view_node.with_semantic_key(#key)),
    );
    Ok(quote! {{
        let __tela_dsl_body = #body?;
        let mut __tela_dsl_primitive = #dsl::__private::UiNode::new(#node_kind);
        #attrs
        let __tela_dsl_view_node = #build.container(__tela_dsl_primitive, __tela_dsl_body)?;
        Ok(#dsl::ViewChild::view_node(#keyed))
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
    if element.children.iter().any(is_directive) {
        return Err(Error::new(
            element.name.span(),
            "Fragment cannot carry @provide, @inject, or @watch; use a real parent node",
        ));
    }
    let body = generate_body(&element.children, build, dsl, scopes, BodyScope::Node)?;
    let site = site(dsl);
    Ok(quote! {{
        let __tela_dsl_body = #body?;
        #build.fragment(__tela_dsl_body, #site)
    }})
}

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
    if element.children.iter().any(is_directive) {
        return Err(Error::new(
            element.name.span(),
            "ActionTarget cannot carry @provide, @inject, or @watch; put directives on a real child scope",
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
    let body = generate_body(&element.children, build, dsl, scopes, BodyScope::Node)?;
    let mut registrations = Vec::new();
    for attribute in &element.attributes {
        reject_closure(&attribute.value)?;
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
            "on_input" => registrations.push(generate_text_action_registration(
                &attribute.value,
                "on_input_at",
                dsl,
                site,
            )?),
            "on_submit" => registrations.push(generate_text_action_registration(
                &attribute.value,
                "on_submit_at",
                dsl,
                site,
            )?),
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

fn generate_text_action_registration(
    expression: &Expr,
    method: &str,
    dsl: &TokenStream2,
    site: TokenStream2,
) -> Result<TokenStream2> {
    let method = Ident::new(method, Span::call_site());
    if is_with_context_call(expression) {
        return Ok(quote! {
            __tela_dsl_target = __tela_dsl_target.#method(#expression, #site);
        });
    }
    if !matches!(expression, Expr::Path(ExprPath { .. })) {
        return Err(Error::new(
            expression.span(),
            "on_input and on_submit require a function path or with_context(value, mapper)",
        ));
    }
    Ok(quote! {
        __tela_dsl_target = {
            let __tela_dsl_mapper: fn(String) -> _ = #expression;
            __tela_dsl_target.#method(
                #dsl::TextActionMap::unary(__tela_dsl_mapper),
                #site,
            )
        };
    })
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
    let item_body = generate_body(
        &element.children,
        build,
        dsl,
        &mut item_scopes,
        BodyScope::Node,
    )?;
    let item_site = site(dsl);
    let loop_body = quote! {
        let __tela_dsl_item_body = #item_body?;
        let __tela_dsl_item = #build.for_item(
            __tela_dsl_item_body,
            &(#key),
            #item_site,
        )?;
        __tela_dsl_items.push(__tela_dsl_item);
    };

    if !virtual_list {
        let collection_scope = scopes.allocate(element.name.span())?;
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
    let collection_scope = scopes.allocate(element.name.span())?;
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

fn generate_text(element: &Element, icon: bool, dsl: &TokenStream2) -> Result<TokenStream2> {
    if element.children.iter().any(is_directive) {
        return Err(Error::new(
            element.name.span(),
            "Text and Icon directives must be declared on an enclosing real node",
        ));
    }
    let mut allowed = COMMON_ATTRIBUTES.to_vec();
    allowed.extend(["value", "font", "font_size", "line_height", "color"]);
    validate_attributes(&element.attributes, &allowed)?;
    let value = text_value(element)?;
    let font = attribute(&element.attributes, "font")
        .map(|attribute| &attribute.value)
        .map_or_else(
            || {
                if icon {
                    quote!(#dsl::__private::TextStyleRef::icon())
                } else {
                    quote!(#dsl::__private::TextStyleRef::body())
                }
            },
            |value| quote!(#value),
        );
    let font_size = attribute(&element.attributes, "font_size")
        .map(|attribute| &attribute.value)
        .map_or_else(|| quote!(14.0_f32), |value| quote!(#value));
    let line_height = attribute(&element.attributes, "line_height")
        .map(|attribute| &attribute.value)
        .map_or_else(|| quote!(20.0_f32), |value| quote!(#value));
    let color = attribute(&element.attributes, "color")
        .map(|attribute| &attribute.value)
        .map_or_else(
            || quote!(#dsl::__private::Color::BLACK),
            |value| quote!(#value),
        );
    let attrs = generate_node_attributes(&element.attributes, dsl)?;
    let key = attribute(&element.attributes, "key").map(|attribute| &attribute.value);
    let keyed = key.map_or_else(
        || quote!(__tela_dsl_node),
        |key| quote!(__tela_dsl_node.with_semantic_key(#key)),
    );
    Ok(quote! {{
        let mut __tela_dsl_primitive = #dsl::__private::UiNode::new(#dsl::__private::NodeKind::Text)
            .with_content(#dsl::__private::ContentConcern::Text(#dsl::__private::TextContent {
                text: (#value).into(),
                font: #font,
                font_size: #font_size,
                line_height: #line_height,
                color: #color,
            }));
        #attrs
        let __tela_dsl_node = #dsl::ViewNode::opaque(__tela_dsl_primitive);
        Ok(#dsl::ViewChild::view_node(#keyed))
    }})
}

fn generate_image(element: &Element, dsl: &TokenStream2) -> Result<TokenStream2> {
    if !element.children.is_empty() {
        return Err(Error::new(
            element.name.span(),
            "Image uses texture={...} and cannot have child nodes",
        ));
    }
    let mut allowed = COMMON_ATTRIBUTES.to_vec();
    allowed.push("texture");
    validate_attributes(&element.attributes, &allowed)?;
    let texture = required_attribute(&element.attributes, "texture", element.name.span())?;
    let attrs = generate_node_attributes(&element.attributes, dsl)?;
    let key = attribute(&element.attributes, "key").map(|attribute| &attribute.value);
    let keyed = key.map_or_else(
        || quote!(__tela_dsl_node),
        |key| quote!(__tela_dsl_node.with_semantic_key(#key)),
    );
    Ok(quote! {{
        let mut __tela_dsl_primitive = #dsl::__private::UiNode::new(#dsl::__private::NodeKind::Image)
            .with_content(#dsl::__private::ContentConcern::Image(#dsl::__private::ImageContent {
                texture: (#texture).into(),
            }));
        #attrs
        let __tela_dsl_node = #dsl::ViewNode::opaque(__tela_dsl_primitive);
        Ok(#dsl::ViewChild::view_node(#keyed))
    }})
}

fn generate_node_attributes(attributes: &[Attribute], dsl: &TokenStream2) -> Result<TokenStream2> {
    let mut layout_fields = Vec::new();
    let mut visual_fields = Vec::new();
    let mut interact_fields = Vec::new();

    for attribute in attributes {
        let value = &attribute.value;
        match attribute.name.to_string().as_str() {
            "key" | "value" | "font" | "font_size" | "line_height" | "color" | "texture" => {}
            "width" => layout_fields.push(quote!(width: Some(#value),)),
            "height" => layout_fields.push(quote!(height: Some(#value),)),
            "margin" => layout_fields.push(quote!(margin: #value,)),
            "padding" => layout_fields.push(quote!(padding: #value,)),
            "border_width" => layout_fields.push(quote!(border_width: #value,)),
            "gap" => layout_fields.push(quote!(gap: #value,)),
            "cross_align" => layout_fields.push(quote!(cross_align: #value,)),
            "clip" => layout_fields.push(quote!(clip: #value,)),
            "overflow" => layout_fields.push(quote!(overflow: #value,)),
            "grid_item" => layout_fields.push(quote!(grid_item: Some(#value),)),
            "text_constraint" => layout_fields.push(quote!(text_constraint: Some(#value),)),
            "fill" => visual_fields.push(quote!(fill: Some(#value),)),
            "border_color" => visual_fields.push(quote!(border_color: Some(#value),)),
            "border_radius" => visual_fields.push(quote!(border_radius: #value,)),
            "shadow" => visual_fields.push(quote!(shadow: Some(#value),)),
            "draw_order" => visual_fields.push(quote!(draw_order: #value,)),
            "visual_offset" => visual_fields.push(quote!(visual_offset: #value,)),
            "clickable" => interact_fields.push(quote!(clickable: #value,)),
            "hoverable" => interact_fields.push(quote!(hoverable: #value,)),
            "focusable" => interact_fields.push(quote!(focusable: #value,)),
            "tab_index" => interact_fields.push(quote!(tab_index: #value,)),
            "input" => interact_fields.push(quote!(input: Some(#value),)),
            "bind_id" => interact_fields.push(quote! {
                bind_id: Some(#dsl::__private::BindId((#value).into())),
            }),
            "pointer_capture" => interact_fields.push(quote!(pointer_capture: #value,)),
            "gestures" => interact_fields.push(quote!(gestures: #value,)),
            "modal" => interact_fields.push(quote!(modal: #value,)),
            _ => {
                return Err(Error::new(
                    attribute.name.span(),
                    "attribute is not supported by this DSL tag",
                ));
            }
        }
    }

    let layout = (!layout_fields.is_empty()).then(|| {
        quote! {
            __tela_dsl_primitive.layout = Some(#dsl::__private::LayoutConcern {
                #(#layout_fields)*
                ..#dsl::__private::LayoutConcern::default()
            });
        }
    });
    let visual = (!visual_fields.is_empty()).then(|| {
        quote! {
            __tela_dsl_primitive.visual = Some(#dsl::__private::VisualConcern {
                #(#visual_fields)*
                ..#dsl::__private::VisualConcern::default()
            });
        }
    });
    let interact = (!interact_fields.is_empty()).then(|| {
        quote! {
            __tela_dsl_primitive.interact = Some(#dsl::__private::InteractConcern {
                #(#interact_fields)*
                ..#dsl::__private::InteractConcern::default()
            });
        }
    });
    Ok(quote!(#layout #visual #interact))
}

fn text_value(element: &Element) -> Result<&Expr> {
    if let Some(attribute) = attribute(&element.attributes, "value") {
        if !element.children.is_empty() {
            return Err(Error::new(
                element.name.span(),
                "Text/Icon uses either value={...} or one { expression } child, not both",
            ));
        }
        return Ok(&attribute.value);
    }
    match element.children.as_slice() {
        [Item::Expr(value)] => Ok(value),
        [] => Err(Error::new(
            element.name.span(),
            "Text/Icon requires value={...} or one { expression } child",
        )),
        _ => Err(Error::new(
            element.name.span(),
            "Text/Icon accepts exactly one { Rust expression } child",
        )),
    }
}

fn is_directive(item: &Item) -> bool {
    matches!(item, Item::Provide(_) | Item::Inject(_) | Item::Watch(_))
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
        if !allowed.iter().any(|allowed| *allowed == name) {
            return Err(Error::new(
                attribute.name.span(),
                format!("attribute `{name}` is not supported on this DSL tag"),
            ));
        }
        if !seen.insert(name.clone()) {
            return Err(Error::new(
                attribute.name.span(),
                format!("duplicate `{name}` attribute"),
            ));
        }
    }
    Ok(())
}

fn is_with_context_call(expression: &Expr) -> bool {
    let Expr::Call(ExprCall { func, args, .. }) = expression else {
        return false;
    };
    let Expr::Path(path) = func.as_ref() else {
        return false;
    };
    path.path
        .segments
        .last()
        .is_some_and(|segment| segment.ident == "with_context")
        && args.len() == 2
}

fn reject_closure(expression: &Expr) -> Result<()> {
    let mut finder = ClosureFinder { span: None };
    finder.visit_expr(expression);
    finder.span.map_or(Ok(()), |span| {
        Err(Error::new(
            span,
            "DSL action attributes cannot contain closures; use a function path or with_context(value, mapper)",
        ))
    })
}

struct ClosureFinder {
    span: Option<Span>,
}

impl<'ast> Visit<'ast> for ClosureFinder {
    fn visit_expr_closure(&mut self, closure: &'ast ExprClosure) {
        self.span.get_or_insert(closure.span());
        visit::visit_expr_closure(self, closure);
    }
}

#[cfg(test)]
mod tests {
    use proc_macro2::Span;

    use super::{CollectionScopes, UiInput, expand};

    fn parse(source: &str) -> UiInput {
        syn::parse_str(source).expect("test DSL input must parse")
    }

    #[test]
    fn accepts_the_block_style_public_invocation() {
        let input = parse("build { <Text>{\"ready\"}</Text> }");

        assert_eq!(input.items.len(), 1);
    }

    #[test]
    fn rejects_the_removed_comma_invocation() {
        let error = match syn::parse_str::<UiInput>("build, { <Text>{\"ready\"}</Text> }") {
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
    fn rejects_context_directives_in_a_real_node_body() {
        let input =
            parse("build { <Column> @inject(label: String); <Text>{label}</Text> </Column> }");
        let error = match expand(input) {
            Ok(_) => panic!("a real node body must not create a Context scope"),
            Err(error) => error,
        };

        assert!(
            error
                .to_string()
                .contains("@inject is only valid in an explicit ui!(build { ... }) block")
        );
    }

    #[test]
    fn rejects_directives_in_identity_transparent_wrappers() {
        let fragment = parse(
            "build { <Fragment> @watch(count, &state.count); <Text>{count}</Text> </Fragment> }",
        );
        let fragment_error =
            expand(fragment).expect_err("a Fragment cannot own a watch or capability scope");
        assert!(
            fragment_error
                .to_string()
                .contains("Fragment cannot carry @provide, @inject, or @watch")
        );

        let target = parse(
            "build { <ActionTarget action={Action::Save}> @watch(count, &state.count); <Frame clickable={true}><Text>{count}</Text></Frame> </ActionTarget> }",
        );
        let target_error =
            expand(target).expect_err("an ActionTarget cannot own a watch or capability scope");
        assert!(
            target_error
                .to_string()
                .contains("ActionTarget cannot carry @provide, @inject, or @watch")
        );
    }

    #[test]
    fn accepts_a_nested_explicit_ui_scope_inside_a_real_node_body() {
        let input = parse(
            "build { <Column> { ui!(build { @inject(label: String); <Text>{label}</Text> }) } </Column> }",
        );

        assert!(expand(input).is_ok());
    }

    #[test]
    fn collection_scope_overflow_is_a_diagnostic_instead_of_a_duplicate_namespace() {
        let mut scopes = CollectionScopes {
            next: u64::from(u32::MAX),
        };

        assert_eq!(
            scopes
                .allocate(Span::call_site())
                .expect("the final u32 scope remains usable"),
            u32::MAX
        );
        let error = scopes
            .allocate(Span::call_site())
            .expect_err("a scope counter must never wrap or saturate");
        assert!(
            error
                .to_string()
                .contains("too many <For> or <VirtualList> declarations")
        );
    }
}
