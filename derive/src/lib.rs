extern crate proc_macro;
extern crate proc_macro2;
#[macro_use]
extern crate quote;
extern crate failure;
#[macro_use]
extern crate log;
extern crate env_logger;
#[macro_use]
extern crate syn;
#[macro_use]
extern crate html5ever;

mod derive_renderable;
use derive_renderable::*;

use failure::{Error, ResultExt};
use html5ever::parse_fragment;
use html5ever::rcdom::{Handle, NodeData, RcDom};
use html5ever::tendril::{StrTendril, TendrilSink};
use html5ever::QualName;
use proc_macro::TokenStream;
use quote::ToTokens;
use std::path::{Path, PathBuf};

#[derive(Debug, Clone)]
enum TemplateSource {
    Path(PathBuf),
    Source(String),
}

#[derive(Debug, Clone)]
struct TemplateDerivation {
    template_source: TemplateSource,
}

#[proc_macro_derive(WeftTemplate, attributes(template))]
pub fn derive_template(input: TokenStream) -> TokenStream {
    // Theoretically `rustc` provides it's own logging, but we
    // don't know for sure that we're using the same `log` crate. So, just in case?
    env_logger::try_init().unwrap_or_default();
    let ast: syn::DeriveInput = syn::parse(input).unwrap();
    match make_template(ast) {
        Ok(toks) => toks.into(),
        Err(err) => panic!("Error: {:?}", err),
    }
}

fn make_template(item: syn::DeriveInput) -> Result<proc_macro2::TokenStream, Error> {
    info!("Deriving for {}", item.ident);
    trace!("{:#?}", item);
    let config = find_template(&item).context("find template")?;
    let dom = config.load_relative_to(&root_dir())?;

    let impl_body = derive_impl(&dom, item)?;

    Ok(impl_body.into_token_stream())
}

fn parse_path(path: &Path) -> Result<Vec<Handle>, Error> {
    info!("Using template from {:?}", path);
    let root_name = QualName::new(None, ns!(html), local_name!("html"));
    let parser =
        parse_fragment(RcDom::default(), Default::default(), root_name, Vec::new()).from_utf8();

    let dom = parser
        .from_file(&path)
        .with_context(|_| format!("Parsing template from path {:?}", &path))?;

    let content = find_root_from(dom.document)
        .ok_or_else(|| failure::err_msg("Could not locate root of parsed document?"))?;

    Ok(content)
}

fn parse_source(source: &str) -> Result<Vec<Handle>, Error> {
    info!("Using inline template");
    let root_name = QualName::new(None, ns!(html), local_name!("html"));
    let parser = parse_fragment(RcDom::default(), Default::default(), root_name, Vec::new());

    let dom = parser.one(source);
    let content = find_root_from(dom.document)
        .ok_or_else(|| failure::err_msg("Could not locate root of parsed document?"))?;

    Ok(content)
}

fn find_root_from(node: Handle) -> Option<Vec<Handle>> {
    let root_name = QualName::new(None, ns!(html), local_name!("html"));
    match node.data {
        NodeData::Element { ref name, .. } => {
            if name == &root_name {
                return Some(node.children.borrow().clone());
            }
        }
        _ => {}
    }
    let children = node.children.borrow();
    for child in children.iter() {
        if let Some(it) = find_root_from(child.clone()) {
            return Some(it);
        }
    }

    None
}

fn find_template(item: &syn::DeriveInput) -> Result<TemplateDerivation, Error> {
    let attr = item
        .attrs
        .iter()
        .filter_map(|a| a.interpret_meta())
        .inspect(|a| info!("Attribute: {:#?}", a))
        .find(|a| a.name() == "template")
        .ok_or_else(|| failure::err_msg("Could not find template attribute"))?;

    let meta_list = match attr {
        syn::Meta::List(inner) => inner,
        _ => return Err(failure::err_msg("template attribute incorrectly formatted")),
    };

    let mut path = None;
    for meta in meta_list.nested {
        if let syn::NestedMeta::Meta(ref item) = meta {
            if let syn::Meta::NameValue(ref pair) = item {
                match pair.ident.to_string().as_ref() {
                    "path" => if let syn::Lit::Str(ref s) = pair.lit {
                        path = Some(PathBuf::from(s.value()));
                    } else {
                        return Err(failure::err_msg(
                            "template path attribute should be a string",
                        ));
                    },
                    _ => warn!("Unrecognised attribute {:#?}", pair),
                }
            }
        }
    }
    let template_source =
        TemplateSource::Path(path.ok_or_else(|| failure::err_msg("Missing path attribute"))?);
    let res = TemplateDerivation { template_source };

    Ok(res)
}

impl TemplateDerivation {
    fn load_relative_to<P: AsRef<Path>>(&self, root_dir: P) -> Result<Vec<Handle>, Error> {
        match &self.template_source {
            TemplateSource::Path(ref path) => {
                let path = PathBuf::from(root_dir.as_ref()).join(path);
                Ok(parse_path(&path)?)
            }
            TemplateSource::Source(ref source) => Ok(parse_source(source)?),
        }
    }
}

fn root_dir() -> PathBuf {
    std::env::var("CARGO_MANIFEST_DIR")
        .unwrap_or_else(|_| {
            warn!("Environment variable $CARGO_MANIFEST_DIR not set, assuming .");
            ".".into()
        }).into()
}
