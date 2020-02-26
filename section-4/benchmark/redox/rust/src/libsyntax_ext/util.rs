use rustc_parse::validate_attr;
use syntax_pos::Symbol;
use syntax::ast::MetaItem;
use syntax::attr::AttributeTemplate;
use syntax_expand::base::ExtCtxt;

pub fn check_builtin_macro_attribute(ecx: &ExtCtxt<'_>, meta_item: &MetaItem, name: Symbol) {
    // All the built-in macro attributes are "words" at the moment.
    let template = AttributeTemplate::only_word();
    let attr = ecx.attribute(meta_item.clone());
    validate_attr::check_builtin_attribute(ecx.parse_sess, &attr, name, template);
}
