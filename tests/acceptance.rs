//! Приёмка настоящего стороннего языкового сервера: пакет gopls без
//! единой правки в ядре. Отличается от go-pack тем, что сервер здесь —
//! не учебная поделка, а gopls: редакторные действия, отключение и
//! обновление пакета проверяются поверх живого gopls.
//!
//! Пропускаемые испытания требуют переменных окружения:
//! * `KMNV_TEST_GOPLS` — путь к gopls.exe;
//! * Go-тулчейн (`go`) в PATH — gopls без него не анализирует пакет.
//! Запуск: `KMNV_TEST_GOPLS=... PATH=.../go/bin:$PATH cargo test --
//! --ignored` из этого каталога.

use ide_plugin::packs;
use ide_plugin::service::LspService;
use ide_plugin_api::service::{
    DocumentRequest, LanguageNavigation, NavigationKind, NavigationResponse,
};
use ide_plugin_api::{Cancel, LanguageQueries, ListingWorkspace, Rope};

/// Раскладка установленного пакета: манифест и gopls в bin/, включённый
/// в реестре. go.mod рядом — иначе gopls не видит модуль и молчит.
fn install(root: &std::path::Path) {
    let package = root.join("packages").join("gopls-pack");
    std::fs::create_dir_all(package.join("bin")).unwrap();
    std::fs::write(
        root.join("registry.json"),
        r#"{"modules":{"gopls-pack":{"enabled":true}}}"#,
    )
    .unwrap();
    std::fs::write(package.join("kmnv-module.json"), manifest()).unwrap();
    std::fs::write(root.join("go.mod"), "module kmnv.test/acceptance\ngo 1.24\n").unwrap();
    std::fs::copy(
        std::env::var_os("KMNV_TEST_GOPLS").expect("путь к gopls.exe"),
        package
            .join("bin")
            .join(format!("gopls{}", std::env::consts::EXE_SUFFIX)),
    )
    .unwrap();
}

fn manifest() -> String {
    std::fs::read_to_string(concat!(env!("CARGO_MANIFEST_DIR"), "/kmnv-module.json")).unwrap()
}

const SOURCE: &str = r#"package main

import (
    "fmt"
    "os"
)

func greet(name string) string {
    return fmt.Sprintf("hi %s", name)
}

func main() {
    fmt.Println(greet(os.Args[1]))
}
"#;

fn request(root: &std::path::Path, text: &str, offset: usize) -> DocumentRequest {
    DocumentRequest {
        path: root.join("main.go"),
        text: Rope::from(text),
        offset,
        workspace: ListingWorkspace {
            roots: vec![root.to_path_buf()],
            ..Default::default()
        },
        language_version: None,
    }
}

fn offset_of(text: &str, needle: &str) -> usize {
    text.find(needle).unwrap() + 2
}

fn hover_text(contents: &ide_plugin_api::lsp_types::HoverContents) -> String {
    use ide_plugin_api::lsp_types::MarkedString;
    match contents {
        ide_plugin_api::lsp_types::HoverContents::Markup(markup) => markup.value.clone(),
        ide_plugin_api::lsp_types::HoverContents::Scalar(MarkedString::String(text)) => {
            text.clone()
        }
        ide_plugin_api::lsp_types::HoverContents::Scalar(MarkedString::LanguageString(lang)) => {
            lang.value.clone()
        }
        ide_plugin_api::lsp_types::HoverContents::Array(items) => items
            .iter()
            .map(|item| match item {
                MarkedString::String(text) => text.clone(),
                MarkedString::LanguageString(lang) => lang.value.clone(),
            })
            .collect::<Vec<_>>()
            .join("\n"),
    }
}

#[test]
#[ignore = "requires KMNV_TEST_GOPLS and a Go toolchain in PATH"]
fn real_gopls_hovers_and_defines_like_an_editor_asks() {
    let root = tempfile::tempdir().unwrap();
    install(root.path());
    let canonical = root.path().canonicalize().unwrap();
    let spec = packs::server_for(&canonical, "go").expect("пакет найден по языку");
    assert!(spec.program.file_name().unwrap().to_string_lossy().starts_with("gopls"));
    let service = LspService::new(spec.clone(), spec.extensions.clone());
    let cancel = Cancel::new();

    let hover = service
        .navigate(
            &request(&canonical, SOURCE, offset_of(SOURCE, "greet(os")),
            NavigationKind::Hover,
            &cancel,
        )
        .unwrap();
    let NavigationResponse::Hover(Some(hover)) = hover else {
        panic!("hover по вызову обязан ответить");
    };
    assert!(
        hover_text(&hover.contents).contains("func greet(name string) string"),
        "gopls показывает объявление: {:?}",
        hover_text(&hover.contents)
    );

    let defined = service
        .navigate(
            &request(&canonical, SOURCE, offset_of(SOURCE, "greet(os")),
            NavigationKind::Definition,
            &cancel,
        )
        .unwrap();
    let NavigationResponse::Locations(links) = defined else {
        panic!("определение обязано ответить");
    };
    assert_eq!(links[0].target_range.start.line, 7);
    service.close();
}

#[test]
#[ignore = "requires KMNV_TEST_GOPLS and a Go toolchain in PATH"]
fn real_gopls_completes_the_files_own_function() {
    let root = tempfile::tempdir().unwrap();
    install(root.path());
    let canonical = root.path().canonicalize().unwrap();
    let spec = packs::server_for(&canonical, "go").unwrap();
    let service = LspService::new(spec.clone(), spec.extensions.clone());
    let cancel = Cancel::new();
    let completing = r#"package main

func greet(name string) string { name }

func main() { gr }
"#;
    let answer = service
        .navigate(
            &request(&canonical, completing, completing.find("gr }").unwrap() + 2),
            NavigationKind::Completion,
            &cancel,
        )
        .unwrap();
    let NavigationResponse::Completions(items) = answer else {
        panic!("completion обязан ответить");
    };
    let labels: Vec<String> = items.iter().map(|item| item.label.clone()).collect();
    assert!(labels.contains(&"greet".to_owned()), "gopls дополняет своей функцией: {labels:?}");
    service.close();
}

#[test]
#[ignore = "requires KMNV_TEST_GOPLS and a Go toolchain in PATH"]
fn real_gopls_publishes_compile_errors() {
    let root = tempfile::tempdir().unwrap();
    install(root.path());
    let canonical = root.path().canonicalize().unwrap();
    let spec = packs::server_for(&canonical, "go").unwrap();
    let service = LspService::new(spec.clone(), spec.extensions.clone());
    let cancel = Cancel::new();
    let workspace = ListingWorkspace {
        roots: vec![canonical.clone()],
        ..Default::default()
    };
    let unused = r#"package main

import (
    "fmt"
    "strings"
)

func main() {
    fmt.Println("hi")
}
"#;
    // gopls публикует диагностику только после загрузки модуля: греем его
    // навигацией, иначе первая публикация не успевает в дедлайн.
    let warm = service
        .navigate(
            &request(&canonical, unused, unused.find("Println").unwrap() + 2),
            NavigationKind::Hover,
            &cancel,
        )
        .unwrap();
    assert!(matches!(warm, NavigationResponse::Hover(_)));
    let published = service
        .diagnostics(&canonical.join("main.go"), unused, &workspace, None, &cancel)
        .unwrap()
        .expect("диагностика опубликована");
    let messages: Vec<&str> = published.iter().map(|d| d.message.as_str()).collect();
    assert!(
        messages.iter().any(|m| m.contains("not used in this file") || m.contains("not used")),
        "gopls ловит неиспользуемый импорт: {messages:?}",
    );
    assert!(
        !messages.iter().any(|m| m.contains("\"fmt\"")),
        "используемый импорт не трогается: {messages:?}",
    );
    service.close();
}

#[test]
#[ignore = "requires KMNV_TEST_GOPLS and a Go toolchain in PATH"]
fn disabling_the_pack_silences_real_gopls() {
    let root = tempfile::tempdir().unwrap();
    install(root.path());
    let canonical = root.path().canonicalize().unwrap();
    let spec = packs::server_for(&canonical, "go").unwrap();
    let service = LspService::new(spec.clone(), spec.extensions.clone());
    let cancel = Cancel::new();
    let request = request(&canonical, SOURCE, offset_of(SOURCE, "greet(os"));
    assert!(
        matches!(
            service.navigate(&request, NavigationKind::Hover, &cancel),
            Ok(NavigationResponse::Hover(Some(_)))
        ),
        "включённый пакет служит"
    );
    service.disable();
    assert!(
        service
            .navigate(&request, NavigationKind::Hover, &cancel)
            .is_err(),
        "остановленный сервис не отвечает"
    );
    service.close();
}
