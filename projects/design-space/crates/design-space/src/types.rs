//! Type constants.
//!
//! A node type is a `u32`:
//!
//! ```text
//! type = (language << 16) | kind
//! ```
//!
//! Language `0` is reserved for structural/universal kinds. Every language
//! owns its own `kind` namespace; the same number may mean something else in
//! another language.
//!
//! Constants are append-only. Never reuse a retired number.

pub const fn make_type(language: u16, kind: u16) -> u32 {
    ((language as u32) << 16) | kind as u32
}

pub const fn language_of(ty: u32) -> u16 {
    (ty >> 16) as u16
}

pub const fn kind_of(ty: u32) -> u16 {
    (ty & 0xFFFF) as u16
}

/// How a node type is encoded on disk.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Layout {
    Object,
    Design,
    Index,
    Version,
}

pub fn layout_of(ty: u32) -> Layout {
    match ty {
        T_DIRECTORY | T_VIRTUAL_ROOT | T_VIEW_ROOT => Layout::Index,
        T_DESIGN => Layout::Design,
        T_VERSION => Layout::Version,
        _ => Layout::Object,
    }
}

// ---------------------------------------------------------------------------
// Languages
// ---------------------------------------------------------------------------

pub const LANG_UNIVERSAL: u16 = 0x0000;
pub const LANG_C: u16 = 0x0001;
pub const LANG_CPP: u16 = 0x0002;
pub const LANG_RUST: u16 = 0x0003;
pub const LANG_JAVA: u16 = 0x0004;
pub const LANG_KOTLIN: u16 = 0x0005;
pub const LANG_CSHARP: u16 = 0x0006;
pub const LANG_GO: u16 = 0x0007;
pub const LANG_PYTHON: u16 = 0x0008;
pub const LANG_JAVASCRIPT: u16 = 0x0009;
pub const LANG_TYPESCRIPT: u16 = 0x000A;
pub const LANG_JSX: u16 = 0x000B;
pub const LANG_TSX: u16 = 0x000C;
pub const LANG_SWIFT: u16 = 0x000D;
pub const LANG_RUBY: u16 = 0x000E;
pub const LANG_PHP: u16 = 0x000F;
pub const LANG_SCALA: u16 = 0x0010;
pub const LANG_SHELL: u16 = 0x0011;
pub const LANG_SQL: u16 = 0x0012;
pub const LANG_LUA: u16 = 0x0013;
pub const LANG_DART: u16 = 0x0014;
pub const LANG_ELIXIR: u16 = 0x0015;
pub const LANG_HASKELL: u16 = 0x0016;
pub const LANG_TEXT: u16 = 0x0017;
pub const LANG_UNKNOWN: u16 = 0x0018;

// ---------------------------------------------------------------------------
// Universal / structural (language 0)
// ---------------------------------------------------------------------------

pub const T_UNKNOWN: u32 = 0x0000_0000;
pub const T_DIRECTORY: u32 = 0x0000_0001;
pub const T_FILE: u32 = 0x0000_0002;
pub const T_SYMLINK: u32 = 0x0000_0003;
pub const T_SUBMODULE: u32 = 0x0000_0004;
pub const T_VIRTUAL_ROOT: u32 = 0x0000_0005;
pub const T_VIEW_ROOT: u32 = 0x0000_0006;
pub const T_DESIGN: u32 = 0x0000_0007;
pub const T_VERSION: u32 = 0x0000_0008;
pub const T_BINARY_FILE: u32 = 0x0000_0009;
pub const T_GENERATED_FILE: u32 = 0x0000_000A;
pub const T_TEXT_FILE: u32 = 0x0000_000B;

// ---------------------------------------------------------------------------
// C (0x0001)
// ---------------------------------------------------------------------------

pub const C_FUNCTION: u32 = 0x0001_0001;
pub const C_VARIABLE: u32 = 0x0001_0002;
pub const C_STRUCT: u32 = 0x0001_0003;
pub const C_UNION: u32 = 0x0001_0004;
pub const C_ENUM: u32 = 0x0001_0005;
pub const C_ENUMERATOR: u32 = 0x0001_0006;
pub const C_TYPEDEF: u32 = 0x0001_0007;
pub const C_MACRO_OBJECT: u32 = 0x0001_0008;
pub const C_MACRO_FUNCTION: u32 = 0x0001_0009;
pub const C_INCLUDE: u32 = 0x0001_000A;

// ---------------------------------------------------------------------------
// C++ (0x0002)
// ---------------------------------------------------------------------------

pub const CPP_NAMESPACE: u32 = 0x0002_0001;
pub const CPP_CLASS: u32 = 0x0002_0002;
pub const CPP_STRUCT: u32 = 0x0002_0003;
pub const CPP_UNION: u32 = 0x0002_0004;
pub const CPP_ENUM: u32 = 0x0002_0005;
pub const CPP_ENUM_CLASS: u32 = 0x0002_0006;
pub const CPP_ENUMERATOR: u32 = 0x0002_0007;
pub const CPP_FUNCTION: u32 = 0x0002_0008;
pub const CPP_METHOD: u32 = 0x0002_0009;
pub const CPP_CONSTRUCTOR: u32 = 0x0002_000A;
pub const CPP_DESTRUCTOR: u32 = 0x0002_000B;
pub const CPP_OPERATOR: u32 = 0x0002_000C;
pub const CPP_FIELD: u32 = 0x0002_000D;
pub const CPP_STATIC_FIELD: u32 = 0x0002_000E;
pub const CPP_GLOBAL_VARIABLE: u32 = 0x0002_000F;
pub const CPP_CONSTANT: u32 = 0x0002_0010;
pub const CPP_TYPEDEF: u32 = 0x0002_0011;
pub const CPP_TYPE_ALIAS: u32 = 0x0002_0012;
pub const CPP_TEMPLATE_CLASS: u32 = 0x0002_0013;
pub const CPP_TEMPLATE_FUNCTION: u32 = 0x0002_0014;
pub const CPP_CONCEPT: u32 = 0x0002_0015;
pub const CPP_MACRO_OBJECT: u32 = 0x0002_0016;
pub const CPP_MACRO_FUNCTION: u32 = 0x0002_0017;
pub const CPP_INCLUDE: u32 = 0x0002_0018;
pub const CPP_USING_DECLARATION: u32 = 0x0002_0019;
pub const CPP_LAMBDA: u32 = 0x0002_001A;
pub const CPP_FRIEND_CLASS: u32 = 0x0002_001B;
pub const CPP_FRIEND_FUNCTION: u32 = 0x0002_001C;
pub const CPP_STATIC_ASSERT: u32 = 0x0002_001D;
pub const CPP_PREPROC_CONDITIONAL: u32 = 0x0002_001E;

// ---------------------------------------------------------------------------
// Rust (0x0003)
// ---------------------------------------------------------------------------

pub const RUST_MODULE: u32 = 0x0003_0001;
pub const RUST_STRUCT: u32 = 0x0003_0002;
pub const RUST_ENUM: u32 = 0x0003_0003;
pub const RUST_UNION: u32 = 0x0003_0004;
pub const RUST_TRAIT: u32 = 0x0003_0005;
pub const RUST_IMPL_BLOCK: u32 = 0x0003_0006;
pub const RUST_TRAIT_IMPL: u32 = 0x0003_0007;
pub const RUST_FUNCTION: u32 = 0x0003_0008;
pub const RUST_METHOD: u32 = 0x0003_0009;
pub const RUST_ASSOC_FN: u32 = 0x0003_000A;
pub const RUST_FIELD: u32 = 0x0003_000B;
pub const RUST_ENUM_VARIANT: u32 = 0x0003_000C;
pub const RUST_CONST: u32 = 0x0003_000D;
pub const RUST_STATIC: u32 = 0x0003_000E;
pub const RUST_TYPE_ALIAS: u32 = 0x0003_000F;
pub const RUST_ASSOC_TYPE: u32 = 0x0003_0010;
pub const RUST_MACRO_RULES: u32 = 0x0003_0011;
pub const RUST_PROC_MACRO: u32 = 0x0003_0012;
pub const RUST_USE: u32 = 0x0003_0013;
pub const RUST_ATTRIBUTE: u32 = 0x0003_0014;

// ---------------------------------------------------------------------------
// Java (0x0004)
// ---------------------------------------------------------------------------

pub const JAVA_PACKAGE: u32 = 0x0004_0001;
pub const JAVA_MODULE: u32 = 0x0004_0002;
pub const JAVA_CLASS: u32 = 0x0004_0003;
pub const JAVA_INTERFACE: u32 = 0x0004_0004;
pub const JAVA_ENUM: u32 = 0x0004_0005;
pub const JAVA_ANNOTATION_TYPE: u32 = 0x0004_0006;
pub const JAVA_RECORD: u32 = 0x0004_0007;
pub const JAVA_FIELD: u32 = 0x0004_0008;
pub const JAVA_METHOD: u32 = 0x0004_0009;
pub const JAVA_CONSTRUCTOR: u32 = 0x0004_000A;
pub const JAVA_STATIC_INITIALIZER: u32 = 0x0004_000B;
pub const JAVA_INSTANCE_INITIALIZER: u32 = 0x0004_000C;
pub const JAVA_ENUM_CONSTANT: u32 = 0x0004_000D;
pub const JAVA_ANNOTATION: u32 = 0x0004_000E;
pub const JAVA_TYPE_PARAMETER: u32 = 0x0004_000F;

// ---------------------------------------------------------------------------
// Kotlin (0x0005)
// ---------------------------------------------------------------------------

pub const KOTLIN_PACKAGE: u32 = 0x0005_0001;
pub const KOTLIN_CLASS: u32 = 0x0005_0002;
pub const KOTLIN_INTERFACE: u32 = 0x0005_0003;
pub const KOTLIN_OBJECT: u32 = 0x0005_0004;
pub const KOTLIN_ENUM: u32 = 0x0005_0005;
pub const KOTLIN_ENUM_ENTRY: u32 = 0x0005_0006;
pub const KOTLIN_FUNCTION: u32 = 0x0005_0007;
pub const KOTLIN_METHOD: u32 = 0x0005_0008;
pub const KOTLIN_CONSTRUCTOR: u32 = 0x0005_0009;
pub const KOTLIN_PROPERTY: u32 = 0x0005_000A;
pub const KOTLIN_TYPE_ALIAS: u32 = 0x0005_000B;
pub const KOTLIN_EXTENSION_FUNCTION: u32 = 0x0005_000C;

// ---------------------------------------------------------------------------
// C# (0x0006)
// ---------------------------------------------------------------------------

pub const CSHARP_NAMESPACE: u32 = 0x0006_0001;
pub const CSHARP_CLASS: u32 = 0x0006_0002;
pub const CSHARP_INTERFACE: u32 = 0x0006_0003;
pub const CSHARP_STRUCT: u32 = 0x0006_0004;
pub const CSHARP_RECORD: u32 = 0x0006_0005;
pub const CSHARP_ENUM: u32 = 0x0006_0006;
pub const CSHARP_DELEGATE: u32 = 0x0006_0007;
pub const CSHARP_FIELD: u32 = 0x0006_0008;
pub const CSHARP_PROPERTY: u32 = 0x0006_0009;
pub const CSHARP_EVENT: u32 = 0x0006_000A;
pub const CSHARP_METHOD: u32 = 0x0006_000B;
pub const CSHARP_CONSTRUCTOR: u32 = 0x0006_000C;
pub const CSHARP_DESTRUCTOR: u32 = 0x0006_000D;
pub const CSHARP_OPERATOR: u32 = 0x0006_000E;
pub const CSHARP_ENUM_MEMBER: u32 = 0x0006_000F;
pub const CSHARP_USING: u32 = 0x0006_0010;

// ---------------------------------------------------------------------------
// Go (0x0007)
// ---------------------------------------------------------------------------

pub const GO_PACKAGE: u32 = 0x0007_0001;
pub const GO_IMPORT: u32 = 0x0007_0002;
pub const GO_STRUCT: u32 = 0x0007_0003;
pub const GO_INTERFACE: u32 = 0x0007_0004;
pub const GO_FUNCTION: u32 = 0x0007_0005;
pub const GO_METHOD: u32 = 0x0007_0006;
pub const GO_FIELD: u32 = 0x0007_0007;
pub const GO_CONST: u32 = 0x0007_0008;
pub const GO_VARIABLE: u32 = 0x0007_0009;
pub const GO_TYPE_ALIAS: u32 = 0x0007_000A;
pub const GO_TYPE_DEFINITION: u32 = 0x0007_000B;

// ---------------------------------------------------------------------------
// Python (0x0008)
// ---------------------------------------------------------------------------

pub const PYTHON_MODULE: u32 = 0x0008_0001;
pub const PYTHON_PACKAGE: u32 = 0x0008_0002;
pub const PYTHON_CLASS: u32 = 0x0008_0003;
pub const PYTHON_FUNCTION: u32 = 0x0008_0004;
pub const PYTHON_METHOD: u32 = 0x0008_0005;
pub const PYTHON_STATIC_METHOD: u32 = 0x0008_0006;
pub const PYTHON_CLASS_METHOD: u32 = 0x0008_0007;
pub const PYTHON_PROPERTY: u32 = 0x0008_0008;
pub const PYTHON_VARIABLE: u32 = 0x0008_0009;
pub const PYTHON_CONSTANT: u32 = 0x0008_000A;
pub const PYTHON_DECORATOR: u32 = 0x0008_000B;
pub const PYTHON_IMPORT: u32 = 0x0008_000C;
pub const PYTHON_LAMBDA: u32 = 0x0008_000D;

// ---------------------------------------------------------------------------
// JavaScript (0x0009)
// ---------------------------------------------------------------------------

pub const JS_MODULE: u32 = 0x0009_0001;
pub const JS_CLASS: u32 = 0x0009_0002;
pub const JS_FUNCTION: u32 = 0x0009_0003;
pub const JS_METHOD: u32 = 0x0009_0004;
pub const JS_ARROW_FUNCTION: u32 = 0x0009_0005;
pub const JS_VARIABLE: u32 = 0x0009_0006;
pub const JS_CONSTANT: u32 = 0x0009_0007;
pub const JS_PROPERTY: u32 = 0x0009_0008;
pub const JS_GETTER: u32 = 0x0009_0009;
pub const JS_SETTER: u32 = 0x0009_000A;
pub const JS_IMPORT: u32 = 0x0009_000B;
pub const JS_EXPORT: u32 = 0x0009_000C;
pub const JS_DECORATOR: u32 = 0x0009_000D;

// ---------------------------------------------------------------------------
// TypeScript (0x000A)
// ---------------------------------------------------------------------------

pub const TS_MODULE: u32 = 0x000A_0001;
pub const TS_CLASS: u32 = 0x000A_0002;
pub const TS_FUNCTION: u32 = 0x000A_0003;
pub const TS_METHOD: u32 = 0x000A_0004;
pub const TS_ARROW_FUNCTION: u32 = 0x000A_0005;
pub const TS_VARIABLE: u32 = 0x000A_0006;
pub const TS_CONSTANT: u32 = 0x000A_0007;
pub const TS_PROPERTY: u32 = 0x000A_0008;
pub const TS_GETTER: u32 = 0x000A_0009;
pub const TS_SETTER: u32 = 0x000A_000A;
pub const TS_INTERFACE: u32 = 0x000A_000B;
pub const TS_TYPE_ALIAS: u32 = 0x000A_000C;
pub const TS_ENUM: u32 = 0x000A_000D;
pub const TS_ENUM_MEMBER: u32 = 0x000A_000E;
pub const TS_NAMESPACE: u32 = 0x000A_000F;
pub const TS_IMPORT: u32 = 0x000A_0010;
pub const TS_EXPORT: u32 = 0x000A_0011;
pub const TS_DECORATOR: u32 = 0x000A_0012;

// ---------------------------------------------------------------------------
// Swift (0x000D)
// ---------------------------------------------------------------------------

pub const SWIFT_MODULE: u32 = 0x000D_0001;
pub const SWIFT_CLASS: u32 = 0x000D_0002;
pub const SWIFT_STRUCT: u32 = 0x000D_0003;
pub const SWIFT_ENUM: u32 = 0x000D_0004;
pub const SWIFT_PROTOCOL: u32 = 0x000D_0005;
pub const SWIFT_EXTENSION: u32 = 0x000D_0006;
pub const SWIFT_FUNCTION: u32 = 0x000D_0007;
pub const SWIFT_METHOD: u32 = 0x000D_0008;
pub const SWIFT_INITIALIZER: u32 = 0x000D_0009;
pub const SWIFT_PROPERTY: u32 = 0x000D_000A;
pub const SWIFT_TYPE_ALIAS: u32 = 0x000D_000B;
pub const SWIFT_ENUM_CASE: u32 = 0x000D_000C;

// ---------------------------------------------------------------------------
// Ruby (0x000E)
// ---------------------------------------------------------------------------

pub const RUBY_MODULE: u32 = 0x000E_0001;
pub const RUBY_CLASS: u32 = 0x000E_0002;
pub const RUBY_METHOD: u32 = 0x000E_0003;
pub const RUBY_SINGLETON_METHOD: u32 = 0x000E_0004;
pub const RUBY_CONSTANT: u32 = 0x000E_0005;
pub const RUBY_VARIABLE: u32 = 0x000E_0006;
pub const RUBY_ATTRIBUTE: u32 = 0x000E_0007;

// ---------------------------------------------------------------------------
// PHP (0x000F)
// ---------------------------------------------------------------------------

pub const PHP_NAMESPACE: u32 = 0x000F_0001;
pub const PHP_CLASS: u32 = 0x000F_0002;
pub const PHP_INTERFACE: u32 = 0x000F_0003;
pub const PHP_TRAIT: u32 = 0x000F_0004;
pub const PHP_ENUM: u32 = 0x000F_0005;
pub const PHP_FUNCTION: u32 = 0x000F_0006;
pub const PHP_METHOD: u32 = 0x000F_0007;
pub const PHP_PROPERTY: u32 = 0x000F_0008;
pub const PHP_CONSTANT: u32 = 0x000F_0009;
pub const PHP_ENUM_CASE: u32 = 0x000F_000A;

// ---------------------------------------------------------------------------
// Scala (0x0010)
// ---------------------------------------------------------------------------

pub const SCALA_PACKAGE: u32 = 0x0010_0001;
pub const SCALA_CLASS: u32 = 0x0010_0002;
pub const SCALA_TRAIT: u32 = 0x0010_0003;
pub const SCALA_OBJECT: u32 = 0x0010_0004;
pub const SCALA_ENUM: u32 = 0x0010_0005;
pub const SCALA_DEF: u32 = 0x0010_0006;
pub const SCALA_VAL: u32 = 0x0010_0007;
pub const SCALA_VAR: u32 = 0x0010_0008;
pub const SCALA_TYPE_ALIAS: u32 = 0x0010_0009;

// ---------------------------------------------------------------------------
// Text (0x0017)
// ---------------------------------------------------------------------------

pub const TEXT_FILE: u32 = 0x0017_0001;
pub const TEXT_SECTION: u32 = 0x0017_0002;

// ---------------------------------------------------------------------------
// Names
// ---------------------------------------------------------------------------

pub const fn language_name(language: u16) -> &'static str {
    match language {
        LANG_UNIVERSAL => "universal",
        LANG_C => "c",
        LANG_CPP => "cpp",
        LANG_RUST => "rust",
        LANG_JAVA => "java",
        LANG_KOTLIN => "kotlin",
        LANG_CSHARP => "csharp",
        LANG_GO => "go",
        LANG_PYTHON => "python",
        LANG_JAVASCRIPT => "javascript",
        LANG_TYPESCRIPT => "typescript",
        LANG_JSX => "jsx",
        LANG_TSX => "tsx",
        LANG_SWIFT => "swift",
        LANG_RUBY => "ruby",
        LANG_PHP => "php",
        LANG_SCALA => "scala",
        LANG_SHELL => "shell",
        LANG_SQL => "sql",
        LANG_LUA => "lua",
        LANG_DART => "dart",
        LANG_ELIXIR => "elixir",
        LANG_HASKELL => "haskell",
        LANG_TEXT => "text",
        _ => "unknown",
    }
}

pub fn type_name(ty: u32) -> Option<&'static str> {
    Some(match ty {
        T_UNKNOWN => "unknown",
        T_DIRECTORY => "directory",
        T_FILE => "file",
        T_SYMLINK => "symlink",
        T_SUBMODULE => "submodule",
        T_VIRTUAL_ROOT => "virtual_root",
        T_VIEW_ROOT => "view_root",
        T_DESIGN => "design",
        T_VERSION => "version",
        T_BINARY_FILE => "binary_file",
        T_GENERATED_FILE => "generated_file",
        T_TEXT_FILE => "text_file",

        C_FUNCTION => "c:function",
        C_VARIABLE => "c:variable",
        C_STRUCT => "c:struct",
        C_UNION => "c:union",
        C_ENUM => "c:enum",
        C_ENUMERATOR => "c:enumerator",
        C_TYPEDEF => "c:typedef",
        C_MACRO_OBJECT => "c:macro_object",
        C_MACRO_FUNCTION => "c:macro_function",
        C_INCLUDE => "c:include",

        CPP_NAMESPACE => "cpp:namespace",
        CPP_CLASS => "cpp:class",
        CPP_STRUCT => "cpp:struct",
        CPP_UNION => "cpp:union",
        CPP_ENUM => "cpp:enum",
        CPP_ENUM_CLASS => "cpp:enum_class",
        CPP_ENUMERATOR => "cpp:enumerator",
        CPP_FUNCTION => "cpp:function",
        CPP_METHOD => "cpp:method",
        CPP_CONSTRUCTOR => "cpp:constructor",
        CPP_DESTRUCTOR => "cpp:destructor",
        CPP_OPERATOR => "cpp:operator",
        CPP_FIELD => "cpp:field",
        CPP_STATIC_FIELD => "cpp:static_field",
        CPP_GLOBAL_VARIABLE => "cpp:global_variable",
        CPP_CONSTANT => "cpp:constant",
        CPP_TYPEDEF => "cpp:typedef",
        CPP_TYPE_ALIAS => "cpp:type_alias",
        CPP_TEMPLATE_CLASS => "cpp:template_class",
        CPP_TEMPLATE_FUNCTION => "cpp:template_function",
        CPP_CONCEPT => "cpp:concept",
        CPP_MACRO_OBJECT => "cpp:macro_object",
        CPP_MACRO_FUNCTION => "cpp:macro_function",
        CPP_INCLUDE => "cpp:include",
        CPP_USING_DECLARATION => "cpp:using_declaration",
        CPP_LAMBDA => "cpp:lambda",
        CPP_FRIEND_CLASS => "cpp:friend_class",
        CPP_FRIEND_FUNCTION => "cpp:friend_function",
        CPP_STATIC_ASSERT => "cpp:static_assert",
        CPP_PREPROC_CONDITIONAL => "cpp:preproc_conditional",

        RUST_MODULE => "rust:module",
        RUST_STRUCT => "rust:struct",
        RUST_ENUM => "rust:enum",
        RUST_UNION => "rust:union",
        RUST_TRAIT => "rust:trait",
        RUST_IMPL_BLOCK => "rust:impl_block",
        RUST_TRAIT_IMPL => "rust:trait_impl",
        RUST_FUNCTION => "rust:function",
        RUST_METHOD => "rust:method",
        RUST_ASSOC_FN => "rust:assoc_fn",
        RUST_FIELD => "rust:field",
        RUST_ENUM_VARIANT => "rust:enum_variant",
        RUST_CONST => "rust:const",
        RUST_STATIC => "rust:static",
        RUST_TYPE_ALIAS => "rust:type_alias",
        RUST_ASSOC_TYPE => "rust:assoc_type",
        RUST_MACRO_RULES => "rust:macro_rules",
        RUST_PROC_MACRO => "rust:proc_macro",
        RUST_USE => "rust:use",
        RUST_ATTRIBUTE => "rust:attribute",

        JAVA_PACKAGE => "java:package",
        JAVA_MODULE => "java:module",
        JAVA_CLASS => "java:class",
        JAVA_INTERFACE => "java:interface",
        JAVA_ENUM => "java:enum",
        JAVA_ANNOTATION_TYPE => "java:annotation_type",
        JAVA_RECORD => "java:record",
        JAVA_FIELD => "java:field",
        JAVA_METHOD => "java:method",
        JAVA_CONSTRUCTOR => "java:constructor",
        JAVA_STATIC_INITIALIZER => "java:static_initializer",
        JAVA_INSTANCE_INITIALIZER => "java:instance_initializer",
        JAVA_ENUM_CONSTANT => "java:enum_constant",
        JAVA_ANNOTATION => "java:annotation",
        JAVA_TYPE_PARAMETER => "java:type_parameter",

        KOTLIN_PACKAGE => "kotlin:package",
        KOTLIN_CLASS => "kotlin:class",
        KOTLIN_INTERFACE => "kotlin:interface",
        KOTLIN_OBJECT => "kotlin:object",
        KOTLIN_ENUM => "kotlin:enum",
        KOTLIN_ENUM_ENTRY => "kotlin:enum_entry",
        KOTLIN_FUNCTION => "kotlin:function",
        KOTLIN_METHOD => "kotlin:method",
        KOTLIN_CONSTRUCTOR => "kotlin:constructor",
        KOTLIN_PROPERTY => "kotlin:property",
        KOTLIN_TYPE_ALIAS => "kotlin:type_alias",
        KOTLIN_EXTENSION_FUNCTION => "kotlin:extension_function",

        CSHARP_NAMESPACE => "csharp:namespace",
        CSHARP_CLASS => "csharp:class",
        CSHARP_INTERFACE => "csharp:interface",
        CSHARP_STRUCT => "csharp:struct",
        CSHARP_RECORD => "csharp:record",
        CSHARP_ENUM => "csharp:enum",
        CSHARP_DELEGATE => "csharp:delegate",
        CSHARP_FIELD => "csharp:field",
        CSHARP_PROPERTY => "csharp:property",
        CSHARP_EVENT => "csharp:event",
        CSHARP_METHOD => "csharp:method",
        CSHARP_CONSTRUCTOR => "csharp:constructor",
        CSHARP_DESTRUCTOR => "csharp:destructor",
        CSHARP_OPERATOR => "csharp:operator",
        CSHARP_ENUM_MEMBER => "csharp:enum_member",
        CSHARP_USING => "csharp:using",

        GO_PACKAGE => "go:package",
        GO_IMPORT => "go:import",
        GO_STRUCT => "go:struct",
        GO_INTERFACE => "go:interface",
        GO_FUNCTION => "go:function",
        GO_METHOD => "go:method",
        GO_FIELD => "go:field",
        GO_CONST => "go:const",
        GO_VARIABLE => "go:variable",
        GO_TYPE_ALIAS => "go:type_alias",
        GO_TYPE_DEFINITION => "go:type_definition",

        PYTHON_MODULE => "python:module",
        PYTHON_PACKAGE => "python:package",
        PYTHON_CLASS => "python:class",
        PYTHON_FUNCTION => "python:function",
        PYTHON_METHOD => "python:method",
        PYTHON_STATIC_METHOD => "python:static_method",
        PYTHON_CLASS_METHOD => "python:class_method",
        PYTHON_PROPERTY => "python:property",
        PYTHON_VARIABLE => "python:variable",
        PYTHON_CONSTANT => "python:constant",
        PYTHON_DECORATOR => "python:decorator",
        PYTHON_IMPORT => "python:import",
        PYTHON_LAMBDA => "python:lambda",

        JS_MODULE => "javascript:module",
        JS_CLASS => "javascript:class",
        JS_FUNCTION => "javascript:function",
        JS_METHOD => "javascript:method",
        JS_ARROW_FUNCTION => "javascript:arrow_function",
        JS_VARIABLE => "javascript:variable",
        JS_CONSTANT => "javascript:constant",
        JS_PROPERTY => "javascript:property",
        JS_GETTER => "javascript:getter",
        JS_SETTER => "javascript:setter",
        JS_IMPORT => "javascript:import",
        JS_EXPORT => "javascript:export",
        JS_DECORATOR => "javascript:decorator",

        TS_MODULE => "typescript:module",
        TS_CLASS => "typescript:class",
        TS_FUNCTION => "typescript:function",
        TS_METHOD => "typescript:method",
        TS_ARROW_FUNCTION => "typescript:arrow_function",
        TS_VARIABLE => "typescript:variable",
        TS_CONSTANT => "typescript:constant",
        TS_PROPERTY => "typescript:property",
        TS_GETTER => "typescript:getter",
        TS_SETTER => "typescript:setter",
        TS_INTERFACE => "typescript:interface",
        TS_TYPE_ALIAS => "typescript:type_alias",
        TS_ENUM => "typescript:enum",
        TS_ENUM_MEMBER => "typescript:enum_member",
        TS_NAMESPACE => "typescript:namespace",
        TS_IMPORT => "typescript:import",
        TS_EXPORT => "typescript:export",
        TS_DECORATOR => "typescript:decorator",

        SWIFT_MODULE => "swift:module",
        SWIFT_CLASS => "swift:class",
        SWIFT_STRUCT => "swift:struct",
        SWIFT_ENUM => "swift:enum",
        SWIFT_PROTOCOL => "swift:protocol",
        SWIFT_EXTENSION => "swift:extension",
        SWIFT_FUNCTION => "swift:function",
        SWIFT_METHOD => "swift:method",
        SWIFT_INITIALIZER => "swift:initializer",
        SWIFT_PROPERTY => "swift:property",
        SWIFT_TYPE_ALIAS => "swift:type_alias",
        SWIFT_ENUM_CASE => "swift:enum_case",

        RUBY_MODULE => "ruby:module",
        RUBY_CLASS => "ruby:class",
        RUBY_METHOD => "ruby:method",
        RUBY_SINGLETON_METHOD => "ruby:singleton_method",
        RUBY_CONSTANT => "ruby:constant",
        RUBY_VARIABLE => "ruby:variable",
        RUBY_ATTRIBUTE => "ruby:attribute",

        PHP_NAMESPACE => "php:namespace",
        PHP_CLASS => "php:class",
        PHP_INTERFACE => "php:interface",
        PHP_TRAIT => "php:trait",
        PHP_ENUM => "php:enum",
        PHP_FUNCTION => "php:function",
        PHP_METHOD => "php:method",
        PHP_PROPERTY => "php:property",
        PHP_CONSTANT => "php:constant",
        PHP_ENUM_CASE => "php:enum_case",

        SCALA_PACKAGE => "scala:package",
        SCALA_CLASS => "scala:class",
        SCALA_TRAIT => "scala:trait",
        SCALA_OBJECT => "scala:object",
        SCALA_ENUM => "scala:enum",
        SCALA_DEF => "scala:def",
        SCALA_VAL => "scala:val",
        SCALA_VAR => "scala:var",
        SCALA_TYPE_ALIAS => "scala:type_alias",

        TEXT_FILE => "text:file",
        TEXT_SECTION => "text:section",

        _ => return None,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn type_encoding_roundtrips() {
        let ty = make_type(LANG_CPP, 0x0002);
        assert_eq!(ty, CPP_CLASS);
        assert_eq!(language_of(ty), LANG_CPP);
        assert_eq!(kind_of(ty), 0x0002);
    }

    #[test]
    fn layouts_are_stable() {
        assert_eq!(layout_of(T_DIRECTORY), Layout::Index);
        assert_eq!(layout_of(T_VIRTUAL_ROOT), Layout::Index);
        assert_eq!(layout_of(T_VIEW_ROOT), Layout::Index);
        assert_eq!(layout_of(T_DESIGN), Layout::Design);
        assert_eq!(layout_of(T_VERSION), Layout::Version);
        assert_eq!(layout_of(CPP_CLASS), Layout::Object);
        assert_eq!(layout_of(T_FILE), Layout::Object);
    }

    #[test]
    fn names_are_available_for_known_types() {
        assert_eq!(type_name(CPP_CLASS), Some("cpp:class"));
        assert_eq!(type_name(JAVA_INTERFACE), Some("java:interface"));
        assert_eq!(type_name(T_DIRECTORY), Some("directory"));
        assert_eq!(type_name(0xFFFF_FFFF), None);
    }
}
