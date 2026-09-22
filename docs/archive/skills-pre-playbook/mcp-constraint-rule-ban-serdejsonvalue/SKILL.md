---
name: mcp-constraint-rule-ban-serdejsonvalue
description: (no description)
disable-model-invocation: true
---

# Constraint: Typed JSON Structures Only

## Rule
NEVER use `serde_json::Value` or `serde_json::json!()` macro for data structures, API responses, configurations, or any persistent data handling.

## Required Approach
ALWAYS create properly typed structs and enums with concrete types and appropriate serde derive macros instead.

## Enforcement
- ❌ **Banned**: `serde_json::Value`, `Value`, `serde_json::json!()`
- ❌ **Banned**: `Map<String, Value>`, `Vec<Value>`, or any `Value` composition
- ✅ **Required**: Concrete structs with `#[derive(Serialize, Deserialize)]`
- ✅ **Required**: Proper enum types for variants
- ✅ **Required**: `Option<T>` for nullable fields
- ✅ **Required**: Newtype wrappers for IDs, timestamps, etc.

## Examples

### ❌ WRONG - Using serde_json::Value
```rust
// DON'T DO THIS
let data = serde_json::json!({
    "user_id": user_id,
    "event_type": "expedition_complete",
    "data": some_dynamic_data
});

struct GameEvent {
    data: serde_json::Value,  // ❌ BANNED
}
```

### ✅ CORRECT - Proper typed structures
```rust
// DO THIS INSTEAD
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ExpeditionCompleteData {
    pub expedition_id: Ulid,
    pub destination: String,
    pub loot_items: Vec<LootItem>,
    pub completion_time: SystemTime,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct GameEvent {
    pub event_type: EventType,
    pub data: GameEventData,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "type")]
pub enum GameEventData {
    ExpeditionComplete(ExpeditionCompleteData),
    CombatStart(CombatStartData),
    PlayerJoin(PlayerJoinData),
}
```

## Limited Exceptions
ONLY allow `serde_json::Value` in these specific cases WITH explicit justification:

1. **External API Integration**: When consuming truly unknown/dynamic external APIs where you cannot control the schema
2. **Debug/Development**: Temporary debugging code that will be removed
3. **Generic Utilities**: Low-level JSON processing utilities (with clear documentation)

## Rationale
- **Type Safety**: Catch errors at compile time, not runtime
- **Documentation**: Structs serve as living documentation of data shapes  
- **IDE Support**: Better autocomplete, refactoring, and navigation
- **Maintainability**: Changes to data structures are caught by the compiler
- **Performance**: Avoid runtime type checking and dynamic allocation

## Before Implementing
When you encounter a scenario where you might reach for `serde_json::Value`, ALWAYS:
1. Define the exact shape of the data you're working with
2. Create appropriate structs/enums to represent that shape
3. Use serde derive macros for serialization
4. Consider using `#[serde(flatten)]`, `#[serde(tag = "type")]`, or other serde attributes for complex cases

This rule applies to ALL code generation, refactoring, and new feature implementation.
