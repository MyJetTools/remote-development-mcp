fn main() {
    ci_utils::css::CssCompiler::new("./css")
        .add_file("01-common.css")
        .add_file("02-layout.css")
        .add_file("03-panels.css")
        .add_file("04-tables.css")
        .add_file("05-dialog.css")
        .add_file("06-sidebar.css")
        .add_file("99-desktop.css")
        .compile("./public/assets/app.css");
}
