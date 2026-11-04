# Authoring compatibility fixtures

These are the SDK's filter, LFO pan and Delay example outputs (`crates/device-sdk/examples/`;
not the built-in Auto Pan of `devices/auto-pan` or the built-in Delay of `devices/delay`), captured at
642da86 before migrating their authoring APIs for #675. The LFO pan example was called `auto-pan`
then; its fixture changed only its id and name when it was renamed for #768. JSON whitespace is compacted;
all graph, parameter and panel fields are retained. They are not stock definitions.

The regression compares their complete data contracts (connection list order is
irrelevant) and renders feedback/modulation at two sample rates through the same SDK
runtime. Do not regenerate these merely to make an authoring refactor pass.
