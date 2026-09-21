# liquid 0.26 (cobalt-org/liquid-rust)

Liquid template engine for Rust. Pure Rust, no system deps, WASM-compatible.

## Crates

- `liquid` — main entry point (ParserBuilder, object!/value! macros, to_object/to_value)
- `liquid-core` — core traits (Filter, ParseTag, ParseBlock, Renderable, Runtime, ValueView, ObjectView)
- `liquid-lib` — stdlib tags and filters
- `liquid-derive` — proc macros (ParseFilter, FilterReflection, FilterParameters, FromFilterParameters, Display_filter)

## Building a Parser with Custom Extensions

```rust
use liquid::ParserBuilder;
use liquid::partials::{EagerCompiler, InMemorySource};

let mut partials = EagerCompiler::<InMemorySource>::empty();
partials.add("header", "<h1>{{ title }}</h1>");

let parser = ParserBuilder::with_stdlib()
    .filter(MyFilter)       // register custom filter
    .tag(MyTag)             // register custom tag
    .block(MyBlock)         // register custom block
    .partials(partials)     // register partials/snippets
    .build()?;
```

## Custom Filter (no params)

```rust
use liquid_core::{Display_filter, Filter, FilterReflection, ParseFilter, Result, Runtime, Value, ValueView};

#[derive(Clone, ParseFilter, FilterReflection)]
#[filter(name = "money", description = "Format as currency", parsed(MoneyFilterImpl))]
pub struct MoneyFilter;

#[derive(Debug, Default, Display_filter)]
#[name = "money"]
struct MoneyFilterImpl;

impl Filter for MoneyFilterImpl {
    fn evaluate(&self, input: &dyn ValueView, _runtime: &dyn Runtime) -> Result<Value> {
        let s = input.render().to_string();
        Ok(Value::scalar(format!("${s}")))
    }
}
```

## Custom Filter (with params)

```rust
use liquid_core::{
    Display_filter, Expression, Filter, FilterParameters, FilterReflection,
    FromFilterParameters, ParseFilter, Result, Runtime, Value, ValueView,
};

#[derive(Debug, FilterParameters)]
struct MyArgs {
    #[parameter(description = "The size param")]
    size: Option<Expression>,
}

#[derive(Clone, ParseFilter, FilterReflection)]
#[filter(name = "my_filter", description = "...", parameters(MyArgs), parsed(MyFilterImpl))]
pub struct MyFilter;

#[derive(Debug, FromFilterParameters, Display_filter)]
#[name = "my_filter"]
struct MyFilterImpl {
    #[parameters]
    args: MyArgs,
}

impl Filter for MyFilterImpl {
    fn evaluate(&self, input: &dyn ValueView, runtime: &dyn Runtime) -> Result<Value> {
        let args = self.args.evaluate(runtime)?;
        let size = args.size.map(|v| v.render().to_string()).unwrap_or_default();
        // ...
        Ok(Value::scalar("result"))
    }
}
```

## Custom Tag

```rust
use std::io::Write;
use liquid_core::{Language, ParseTag, Renderable, Result, Runtime, TagReflection, TagTokenIter, ValueView};

#[derive(Copy, Clone, Debug, Default)]
pub struct MyTag;

impl TagReflection for MyTag {
    fn tag(&self) -> &'static str { "mytag" }
    fn description(&self) -> &'static str { "Does a thing" }
}

impl ParseTag for MyTag {
    fn parse(&self, mut arguments: TagTokenIter<'_>, _options: &Language) -> Result<Box<dyn Renderable>> {
        arguments.expect_nothing()?;
        Ok(Box::new(MyTagRenderable))
    }
    fn reflection(&self) -> &dyn TagReflection { self }
}

#[derive(Debug)]
struct MyTagRenderable;

impl Renderable for MyTagRenderable {
    fn render_to(&self, writer: &mut dyn Write, runtime: &dyn Runtime) -> Result<()> {
        // NOTE: write! returns io::Error, but Renderable expects liquid_core::Error
        // Must convert manually:
        writer.write_all(b"output").map_err(|e| {
            liquid_core::Error::with_msg("IO error").context("mytag", format!("{e}"))
        })?;
        Ok(())
    }
}
```

## Accessing Runtime Variables in Tags

```rust
// Get nested value like "page.title"
let path = [
    liquid_core::model::ScalarCow::new("page"),
    liquid_core::model::ScalarCow::new("title"),
];
if let Some(val) = runtime.try_get(&path) {
    let s = val.render().to_string();  // requires `use liquid_core::ValueView;`
}
```

## Creating Template Data

```rust
// Inline object
let globals = liquid::object!({
    "name": "World",
    "count": 42,
    "nested": { "key": "value" },
});

// From serde types
let data = MyStruct { field: "value" };
let globals: liquid::Object = liquid::to_object(&data)?;
let val: liquid::model::Value = liquid::model::to_value(&data)?;

// Manual construction
let mut obj = liquid::Object::new();
obj.insert("key".into(), liquid::model::Value::scalar("value"));
```

## Partials (Includes)

```rust
// In templates: {% include "snippet_name" %}
// Or with variables: {% include "snippet_name", var: value %}

// Register partials before building parser:
let mut partials = EagerCompiler::<InMemorySource>::empty();
partials.add("snippet_name", "template source {{ var }}");
```
