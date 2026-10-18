# Authoring compatibility fixtures

These are Suhina's own filter, auto-pan and Delay example outputs, captured at
642da86 before migrating their authoring APIs for #675. JSON whitespace is compacted;
all graph, parameter and panel fields are retained. They are not stock definitions.

The regression compares their complete data contracts (connection list order is
irrelevant) and renders feedback/modulation at two sample rates through the same SDK
runtime. Do not regenerate these merely to make an authoring refactor pass.
