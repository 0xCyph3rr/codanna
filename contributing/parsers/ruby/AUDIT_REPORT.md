# Ruby Parser Symbol Extraction Coverage Report

*Generated: 2025-11-05 16:11:04 UTC*

## Summary
- Nodes in file: 89
- Nodes with symbol extraction: 6
- Symbol kinds extracted: 4

> **Note**: This report tracks nodes that produce indexed symbols for code intelligence.
> For complete grammar coverage, see GRAMMAR_ANALYSIS.md

## Coverage Table

*Showing key nodes relevant for symbol extraction. Status determined by dynamic tracking.*

| Node Type | ID | Status |
|-----------|-----|--------|
| module | 27 | ✅ implemented |
| class | 25 | ✅ implemented |
| singleton_class | 183 | ⚠️ gap |
| method | 163 | ✅ implemented |
| singleton_method | 164 | ✅ implemented |
| assignment | 276 | ✅ implemented |
| operator_assignment | 278 | ⚠️ gap |
| constant | 117 | ⚠️ gap |
| instance_variable | 120 | ⚠️ gap |
| class_variable | 121 | ⚠️ gap |
| global_variable | 122 | ⚠️ gap |
| block | 275 | ⚠️ gap |
| lambda | 325 | ⚠️ gap |
| do_block | 274 | ⚠️ gap |
| if | 34 | ⚠️ gap |
| unless | 35 | ⚠️ gap |
| case | - | ❌ not found |
| when | - | ❌ not found |
| while | - | ❌ not found |
| until | - | ❌ not found |
| for | - | ❌ not found |
| begin | - | ❌ not found |
| rescue | - | ❌ not found |
| ensure | - | ❌ not found |
| call | 265 | ✅ implemented |
| method_call | - | ❌ not found |
| method_parameters | 169 | ⚠️ gap |
| block_parameters | 171 | ⚠️ gap |
| optional_parameter | 180 | ⚠️ gap |
| keyword_parameter | 179 | ⚠️ gap |
| splat_parameter | 175 | ⚠️ gap |
| block_parameter | 178 | ⚠️ gap |
| symbol | - | ❌ not found |
| string | 314 | ⚠️ gap |
| heredoc_beginning | - | ❌ not found |
| attr_reader | - | ❌ not found |
| attr_writer | - | ❌ not found |
| attr_accessor | - | ❌ not found |
| alias | - | ❌ not found |
| undef | - | ❌ not found |
| include | - | ❌ not found |
| extend | - | ❌ not found |
| prepend | - | ❌ not found |

## Legend

- ✅ **implemented**: Node type is recognized and handled by the parser
- ⚠️ **gap**: Node type exists in the grammar but not handled by parser (needs implementation)
- ❌ **not found**: Node type not present in the example file (may need better examples)

## Recommended Actions

### Priority 1: Implementation Gaps
These nodes exist in your code but aren't being captured:

- `singleton_class`: Add parsing logic in parser.rs
- `operator_assignment`: Add parsing logic in parser.rs
- `constant`: Add parsing logic in parser.rs
- `instance_variable`: Add parsing logic in parser.rs
- `class_variable`: Add parsing logic in parser.rs
- `global_variable`: Add parsing logic in parser.rs
- `block`: Add parsing logic in parser.rs
- `lambda`: Add parsing logic in parser.rs
- `do_block`: Add parsing logic in parser.rs
- `if`: Add parsing logic in parser.rs
- `unless`: Add parsing logic in parser.rs
- `method_parameters`: Add parsing logic in parser.rs
- `block_parameters`: Add parsing logic in parser.rs
- `optional_parameter`: Add parsing logic in parser.rs
- `keyword_parameter`: Add parsing logic in parser.rs
- `splat_parameter`: Add parsing logic in parser.rs
- `block_parameter`: Add parsing logic in parser.rs
- `string`: Add parsing logic in parser.rs

### Priority 2: Missing Examples
These nodes aren't in the comprehensive example. Consider:

- `case`: Add example to comprehensive.rb or verify node name
- `when`: Add example to comprehensive.rb or verify node name
- `while`: Add example to comprehensive.rb or verify node name
- `until`: Add example to comprehensive.rb or verify node name
- `for`: Add example to comprehensive.rb or verify node name
- `begin`: Add example to comprehensive.rb or verify node name
- `rescue`: Add example to comprehensive.rb or verify node name
- `ensure`: Add example to comprehensive.rb or verify node name
- `method_call`: Add example to comprehensive.rb or verify node name
- `symbol`: Add example to comprehensive.rb or verify node name
- `heredoc_beginning`: Add example to comprehensive.rb or verify node name
- `attr_reader`: Add example to comprehensive.rb or verify node name
- `attr_writer`: Add example to comprehensive.rb or verify node name
- `attr_accessor`: Add example to comprehensive.rb or verify node name
- `alias`: Add example to comprehensive.rb or verify node name
- `undef`: Add example to comprehensive.rb or verify node name
- `include`: Add example to comprehensive.rb or verify node name
- `extend`: Add example to comprehensive.rb or verify node name
- `prepend`: Add example to comprehensive.rb or verify node name

