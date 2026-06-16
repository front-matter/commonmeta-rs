use serde::Deserialize;

#[derive(Deserialize, Default, Debug)]
struct Root {
    #[serde(rename = "jats:abstract", default)]
    abstract_prefixed: Vec<Inner>,
    #[serde(rename = "abstract", default)]
    abstract_unprefixed: Vec<Inner>,
}

#[derive(Deserialize, Default, Debug)]
struct Inner {
    #[serde(rename = "jats:p", default)]
    p_prefixed: Vec<P>,
    #[serde(rename = "p", default)]
    p_unprefixed: Vec<P>,
}

#[derive(Deserialize, Default, Debug)]
struct P {
    #[serde(rename = "$text", default)]
    #[allow(dead_code)]
    text: String,
}

fn main() {
    let xml = r#"<root>
  <jats:abstract xmlns:jats="http://www.ncbi.nlm.nih.gov/JATS1">
    <jats:p>Hello world</jats:p>
  </jats:abstract>
</root>"#;
    let r: Root = quick_xml::de::from_str(xml).unwrap();
    println!("abstract_prefixed count: {}", r.abstract_prefixed.len());
    println!("abstract_unprefixed count: {}", r.abstract_unprefixed.len());
    if let Some(a) = r.abstract_prefixed.first().or(r.abstract_unprefixed.first()) {
        println!("p_prefixed count: {}", a.p_prefixed.len());
        println!("p_unprefixed count: {}", a.p_unprefixed.len());
    }
}
