#![feature(rustc_private)]
#![deny(warnings)]

extern crate env_logger;
extern crate syntax;

use std::collections::BTreeMap;
use std::env;
use std::error::Error;
use std::fs::File;
use std::io::Write;
use std::path::Path;
use std::path::PathBuf;
use std::cell::RefCell;

use syntax::edition::DEFAULT_EDITION;

use rustdoc::html::markdown::{Markdown, IdMap, ErrorCodes, Playground};

pub struct ErrorMetadata {
    pub description: Option<String>,
}

/// Mapping from error codes to metadata that can be (de)serialized.
pub type ErrorMetadataMap = BTreeMap<String, ErrorMetadata>;

enum OutputFormat {
    HTML(HTMLFormatter),
    Markdown(MarkdownFormatter),
    Unknown(String),
}

impl OutputFormat {
    fn from(format: &str, resource_suffix: &str) -> OutputFormat {
        match &*format.to_lowercase() {
            "html"     => OutputFormat::HTML(HTMLFormatter(RefCell::new(IdMap::new()),
                                                           resource_suffix.to_owned())),
            "markdown" => OutputFormat::Markdown(MarkdownFormatter),
            s          => OutputFormat::Unknown(s.to_owned()),
        }
    }
}

trait Formatter {
    fn header(&self, output: &mut dyn Write) -> Result<(), Box<dyn Error>>;
    fn title(&self, output: &mut dyn Write) -> Result<(), Box<dyn Error>>;
    fn error_code_block(&self, output: &mut dyn Write, info: &ErrorMetadata,
                        err_code: &str) -> Result<(), Box<dyn Error>>;
    fn footer(&self, output: &mut dyn Write) -> Result<(), Box<dyn Error>>;
}

struct HTMLFormatter(RefCell<IdMap>, String);
struct MarkdownFormatter;

impl Formatter for HTMLFormatter {
    fn header(&self, output: &mut dyn Write) -> Result<(), Box<dyn Error>> {
        write!(output, r##"<!DOCTYPE html>
<html>
<head>
<title>Rust Compiler Error Index</title>
<meta charset="utf-8">
<!-- Include rust.css after light.css so its rules take priority. -->
<link rel="stylesheet" type="text/css" href="light{suffix}.css"/>
<link rel="stylesheet" type="text/css" href="rust.css"/>
<style>
.error-undescribed {{
    display: none;
}}
</style>
</head>
<body>
"##, suffix=self.1)?;
        Ok(())
    }

    fn title(&self, output: &mut dyn Write) -> Result<(), Box<dyn Error>> {
        write!(output, "<h1>Rust Compiler Error Index</h1>\n")?;
        Ok(())
    }

    fn error_code_block(&self, output: &mut dyn Write, info: &ErrorMetadata,
                        err_code: &str) -> Result<(), Box<dyn Error>> {
        // Enclose each error in a div so they can be shown/hidden en masse.
        let desc_desc = match info.description {
            Some(_) => "error-described",
            None => "error-undescribed",
        };
        write!(output, "<div class=\"{}\">", desc_desc)?;

        // Error title (with self-link).
        write!(output,
               "<h2 id=\"{0}\" class=\"section-header\"><a href=\"#{0}\">{0}</a></h2>\n",
               err_code)?;

        // Description rendered as markdown.
        match info.description {
            Some(ref desc) => {
                let mut id_map = self.0.borrow_mut();
                let playground = Playground {
                    crate_name: None,
                    url: String::from("https://play.rust-lang.org/"),
                };
                write!(output, "{}",
                    Markdown(desc, &[], &mut id_map,
                             ErrorCodes::Yes, DEFAULT_EDITION, &Some(playground)).to_string())?
            },
            None => write!(output, "<p>No description.</p>\n")?,
        }

        write!(output, "</div>\n")?;
        Ok(())
    }

    fn footer(&self, output: &mut dyn Write) -> Result<(), Box<dyn Error>> {
        write!(output, r##"<script>
function onEach(arr, func) {{
    if (arr && arr.length > 0 && func) {{
        for (var i = 0; i < arr.length; i++) {{
            func(arr[i]);
        }}
    }}
}}

function hasClass(elem, className) {{
    if (elem && className && elem.className) {{
        var elemClass = elem.className;
        var start = elemClass.indexOf(className);
        if (start === -1) {{
            return false;
        }} else if (elemClass.length === className.length) {{
            return true;
        }} else {{
            if (start > 0 && elemClass[start - 1] !== ' ') {{
                return false;
            }}
            var end = start + className.length;
            if (end < elemClass.length && elemClass[end] !== ' ') {{
                return false;
            }}
            return true;
        }}
        if (start > 0 && elemClass[start - 1] !== ' ') {{
            return false;
        }}
        var end = start + className.length;
        if (end < elemClass.length && elemClass[end] !== ' ') {{
            return false;
        }}
        return true;
    }}
    return false;
}}

onEach(document.getElementsByClassName('rust-example-rendered'), function(e) {{
    if (hasClass(e, 'compile_fail')) {{
        e.addEventListener("mouseover", function(event) {{
            e.previousElementSibling.childNodes[0].style.color = '#f00';
        }});
        e.addEventListener("mouseout", function(event) {{
            e.previousElementSibling.childNodes[0].style.color = '';
        }});
    }} else if (hasClass(e, 'ignore')) {{
        e.addEventListener("mouseover", function(event) {{
            e.previousElementSibling.childNodes[0].style.color = '#ff9200';
        }});
        e.addEventListener("mouseout", function(event) {{
            e.previousElementSibling.childNodes[0].style.color = '';
        }});
    }}
}});
</script>
</body>
</html>"##)?;
        Ok(())
    }
}

impl Formatter for MarkdownFormatter {
    #[allow(unused_variables)]
    fn header(&self, output: &mut dyn Write) -> Result<(), Box<dyn Error>> {
        Ok(())
    }

    fn title(&self, output: &mut dyn Write) -> Result<(), Box<dyn Error>> {
        write!(output, "# Rust Compiler Error Index\n")?;
        Ok(())
    }

    fn error_code_block(&self, output: &mut dyn Write, info: &ErrorMetadata,
                        err_code: &str) -> Result<(), Box<dyn Error>> {
        Ok(match info.description {
            Some(ref desc) => write!(output, "## {}\n{}\n", err_code, desc)?,
            None => (),
        })
    }

    #[allow(unused_variables)]
    fn footer(&self, output: &mut dyn Write) -> Result<(), Box<dyn Error>> {
        Ok(())
    }
}

/// Output an HTML page for the errors in `err_map` to `output_path`.
fn render_error_page<T: Formatter>(err_map: &ErrorMetadataMap, output_path: &Path,
                                   formatter: T) -> Result<(), Box<dyn Error>> {
    let mut output_file = File::create(output_path)?;

    formatter.header(&mut output_file)?;
    formatter.title(&mut output_file)?;

    for (err_code, info) in err_map {
        formatter.error_code_block(&mut output_file, info, err_code)?;
    }

    formatter.footer(&mut output_file)
}

fn main_with_result(format: OutputFormat, dst: &Path) -> Result<(), Box<dyn Error>> {
    let long_codes = register_all();
    let mut err_map = BTreeMap::new();
    for (code, desc) in long_codes {
        err_map.insert(code.to_string(), ErrorMetadata {
            description: desc.map(String::from),
        });
    }
    match format {
        OutputFormat::Unknown(s)  => panic!("Unknown output format: {}", s),
        OutputFormat::HTML(h)     => render_error_page(&err_map, dst, h)?,
        OutputFormat::Markdown(m) => render_error_page(&err_map, dst, m)?,
    }
    Ok(())
}

fn parse_args() -> (OutputFormat, PathBuf) {
    let mut args = env::args().skip(1);
    let format = args.next();
    let dst = args.next();
    let resource_suffix = args.next().unwrap_or_else(String::new);
    let format = format.map(|a| OutputFormat::from(&a, &resource_suffix))
                       .unwrap_or(OutputFormat::from("html", &resource_suffix));
    let dst = dst.map(PathBuf::from).unwrap_or_else(|| {
        match format {
            OutputFormat::HTML(..) => PathBuf::from("doc/error-index.html"),
            OutputFormat::Markdown(..) => PathBuf::from("doc/error-index.md"),
            OutputFormat::Unknown(..) => PathBuf::from("<nul>"),
        }
    });
    (format, dst)
}

fn main() {
    env_logger::init();
    let (format, dst) = parse_args();
    let result = syntax::with_default_globals(move || {
        main_with_result(format, &dst)
    });
    if let Err(e) = result {
        panic!("{}", e.description());
    }
}

include!(concat!(env!("OUT_DIR"), "/error_codes.rs"));
