#[cfg(test)]
mod text {
    const STRING: &str = "
mod in_string;
";
    const RAW: &str = r#"
mod in_raw;
"#;
    /*
mod in_comment;
    */
    fn f() {}
}
