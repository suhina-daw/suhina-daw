---
description: How the sample map analyses samples (features), lays them out (PCA and t-SNE, grid assignment), finds similar ones (a linear scan), caches the results, and which crates it uses
read_when: sample map, soundmap crate, audio features, MFCC, spectral centroid, band energy, onset, pitch glide, t-SNE, bhtsne, PCA, UMAP, grid layout, RasterFairy, nearest neighbours, feature cache, kinds, classifier, kind set by hand
type: decision
status: accepted
built: yes
last_verified: 2026-10-21
covers:
  - crates/soundmap/**
  - crates/session/src/soundmap.rs
---

# 0013: The sample map: features, layout and cache

- **Date:** 2026-10-12

## Context

The sample map ([spec](../spec/sample-map.md)) places a library's samples on a plane by how
they sound. That needs, for every sample, a short list of numbers describing its sound
(features); a way to turn tens of thousands of such lists into 2D positions where near means
similar (dimensionality reduction); a grid version of those positions; and a fast way to find
an item's nearest neighbours. Libraries hold up to hundreds of thousands of files, so the
analysis must be cached and incremental, and it must never disturb playback.

The method is well established in open research and software (the spec's *Prior art*):
per-frame spectral features summarised by their mean and spread, standardised, reduced with
t-SNE or UMAP. What remains is choosing the features, the methods and the crates.

## Decision

1. **A crate of its own, `suhina-soundmap` (layer 2, beside `library`)**, depends on `media`
   (decoding) only. The session feeds it the library's files and watches the library's revision
   (`Library::files`, `Library::revision`), so it lays out again when files change. It holds
   the analysis queue and threads, the feature cache, the layouts and the neighbour search. The
   `soundmap.*` commands run on it, held by `Projects` like the library. Nothing in it touches
   the audio thread.
2. **Features, version 2** (57 numbers per item; *amended 2026-10-15, #325*: version 1 had 44
   and read the first 4 seconds as they were). From a mono sum of the file resampled to
   22 050 Hz, starting at its **onset**: the first millisecond within 40 dB of the loudest,
   looked for in the first 4 seconds (leading silence is skipped; a file silent that long is not
   analysed), for 4 seconds. Frames of 1 024 samples with a hop of 256, with a Hann window:
   - 13 MFCCs (40 mel bands, the zeroth coefficient left out, each band at most 80 dB under the
     frame's loudest, so they do not change with the level): mean and standard deviation over
     the frames whose level is within 40 dB of the loudest (26);
   - spectral centroid, spread, flatness, roll-off (85 %) and flux: mean and standard deviation
     over the same frames (10);
   - zero-crossing rate: mean and standard deviation (2);
   - the level envelope, in blocks of 64 samples: attack time (start to peak), decay time (peak
     to −20 dB), effective length (above −40 dB) and crest factor, the last over the sound until
     it has decayed by 20 dB (4);
   - pitch: the median fundamental of the voiced frames in semitones and the share of voiced
     frames, by a YIN-style estimate; and the **glide**, how far the strongest peak under 500 Hz
     falls over the first 0.25 s, over the frames whose energy lies mostly below 500 Hz (the
     sweep of a kick or a tom) (3);
   - the **attack**, the first 30 ms on their own under a Hann window of that length: the
     energy share in five bands (under 120 Hz, 120 to 500 Hz, 0.5 to 2 kHz, 2 to 6 kHz, over
     6 kHz, in dB), the centroid and the flatness (7);
   - the **body**: the same five band shares over the loud frames after the attack, their
     energy summed (5).

   The frame statistics count each frame by its energy, so a quiet tail (a room's, a release,
   a later hit of a loop) counts little beside the sound itself. Times are in log seconds, so
   short sounds are told apart; frequencies in log2 Hz. Each feature is tested on signals whose
   answer is known (a sine's pitch and centroid, noise's flatness, zero crossings and bands, a
   short hit's length, a falling sine's glide, a bright click before a low tone, the same result
   for mono and stereo, across sample rates and levels and after leading silence). The feature
   list has a version; a cache entry of another version is analysed again, never misread (its
   header says the version). Analysing takes as long as version 1 did (about 3 ms for a
   half-second one-shot, 22 ms for a 6-second file, on one core of one machine).
3. **Distances** are Euclidean over features standardised across the scope (each one to mean 0,
   deviation 1). Each of the six groups (MFCC, spectral shape, envelope, pitch, attack, body
   bands) is scaled so that its size does not matter, then weighted: 1 for the four that
   describe the spectrum, ½ for the envelope and the pitch, so two short sharp hits are not near
   whatever their spectrum (*amended 2026-10-15, #325*: the groups were equal).
4. **Layout:** PCA down to 20 dimensions (the covariance matrix's eigenvectors by Jacobi
   rotations, each with a fixed sign), then Barnes-Hut t-SNE (θ 0.5, perplexity 30, or a third
   of the item count when that is smaller; 1 000 iterations), started from the first two
   principal components with a little jitter seeded from the scope's paths. Scopes under 16
   items use the first two components alone. The result is scaled to fit 0–1, keeping its
   aspect. The layout's input is not the features alone but the features with the kinds
   (*Kinds*): each item of a kind is drawn half-way to its kind's centre and moved 1.5 times the
   scope's typical distance from its centre along a direction of its kind's own (17 directions
   at right angles), so each kind forms regions of its own and an item of one kind cannot sit
   among another's; items of no kind stay where their sound puts them. "Similar items" use the
   features alone. t-SNE sums in parallel, so two runs can differ in the last digits; therefore a
   layout is kept, in memory and in `layout-<hash>.bin` beside the caches (the 32 newest), by a
   hash of the scope's items with their sizes, times and kinds set by hand, and the feature
   version: a scope opens with the map it had, and
   changes only when its items do. Measured on one machine: 1 000 items in 0.5 s, 10 000 in
   4.5 s, 50 000 in 18 s.
   *Amended 2026-10-13 (#195), 200 000 items:* the limit of a map is 200 000 items (it was
   100 000), for libraries of that size. The t-SNE settings stay. `bhtsne`'s learning rate
   already grows with the item count (n / 48, at least 50). Early exaggeration is 12 for the
   first 250 epochs. These are the settings that large-scale t-SNE work recommends for keeping
   clusters apart at this size, so nothing is tuned by hand. Whether a UMAP-style layout
   separates kinds better at 200 000 stays open (*Alternatives*). Drawing that many points is the
   view's job: the spec's *Many items* (a density raster above 20 000 points in view, clusters
   labelled without overlap).
5. **Grid:** its columns and rows follow the map view's aspect ratio (`soundmap.layout` takes it
   as `aspect`), and the view makes the grid again when its shape changes. Recursive splitting
   (the RasterFairy method): the cells are cut in two along the
   longer side and the points, sorted along the same axis, are shared between the halves in
   proportion to their cells, until a part holds at most 256 items. Each part's points are
   stretched over its cells and assigned exactly by the Hungarian method (least total squared
   distance from point to cell centre). 50 000 items take about a second.
6. **Neighbours:** "similar items" is a linear scan over the standardised features (under 2 ms
   for 50 000 items); no tree is needed at these sizes. Finding the point under the pointer is
   the view's job.
7. **Kinds** are found before the layout, in feature space, never from positions on the map
   (*amended 2026-10-15, #325*: an item without a name took the kind of its 7 nearest
   neighbours when 4 agreed, and a folder's kind always stood; #303's vote of the nearest named
   items on the 2D map was set aside for this). Names are read as the spec says: the file name,
   then its two nearest folders, the most specific kind in a name winning. Evidence comes in three
   strengths: a kind **set by hand** wins over everything; a kind from the **file name** stands;
   a kind from a **folder** gives way to a clear sound. A light classifier is learned from the
   scope's own items, so no model is shipped: for each kind, its examples are the items whose
   kind is set by hand or comes from the file name (the folder-named ones as well when there are
   fewer than 4), at most 2 000 taken evenly; k-means gives up to 4 centres (one per 6
   examples), started from points spread over the nearer nine tenths of the examples, and a
   centre left with less than 15 % of them is dropped, so a few misnamed files do not make a
   region of their own. The kind's radius is the distance nine in ten of its examples are from
   their nearest centre (at least a quarter of the scope's typical distance from its centre).
   An item without a kind takes the nearest kind when it is within 2.5 radii of it and every kind
   of another family is at least 1.15 times further; a folder's kind gives way when another
   family's kind is within reach and 1.4 times nearer. Closed, open and plain hats, crash, ride
   and cymbal, and snare and rim are families. Classifying 150 000 items takes well under a
   second, on several threads. The kinds set by hand are kept in `kinds.txt` beside the caches:
   a header line, then the kind's id and the file's absolute path per line, sorted; at most
   1 000 000 of them. A malformed line is left out; a file of another header holds none.
8. **Cache** in `<settings>/library/features-<hash>.bin` beside the library's index, one file
   per folder: a version header with the feature version, then per file its path below the
   folder, size, modification time and either its length in seconds and its features (32-bit
   floats) or the reason it could not be analysed. The layout shows only files no longer than
   the length limit, so changing the limit needs no new analysis. An entry whose size or time no longer
   matches the library's index is analysed again. Written to a file beside it and moved over
   it, as the index is. A file that does not parse is ignored and the folder analysed again.
   *Amended 2026-10-19:* new and removed entries are appended to a journal beside it
   (`features-<hash>.log`) and the file is written whole only when the journal passes a quarter
   of its entries; every write runs on a thread of its own, never the window's (a 63 MiB cache
   written and synced there froze the window for 700 ms). Loading reads the file, then the
   journal up to a torn last record, and cuts the journal there, so the records appended after
   a crash follow a whole one and are read at the next start.
9. **Threads:** analysis runs on a pool of half the cores (at least one), started by
   `Library::start`'s caller as indexing is, so tests and the CLI never analyse a user's
   folders unasked. A file larger than 256 MiB is not decoded. Caches are written after every
   256 analysed files of a folder and when the queue empties.
10. **Crates**, both MIT: `realfft` for the transforms and `bhtsne` for t-SNE. MFCC, the
    spectral features, YIN, PCA, the grid split and the Hungarian method are written in the
    crate: each is short, and owning them avoids pulling in a matrix library. `bhtsne` checks
    its inputs with assertions, so its inputs are checked first (perplexity within the item
    count, finite values) and the call is wrapped in `catch_unwind`; a failure falls back to the
    first two principal components.

## Consequences

- A library is analysed once; afterwards maps open from the cache in well under a second for
  ten thousand items, and only new or changed files are analysed.
- Two new dependencies.
- The map is only as good as the features. Versions 1 and 2 favour one-shots (drums, short
  hits); tonal material and loops will need more (chroma, tempo-aware windows) in a later
  version.
- Kinds are only as good as the library's names: the classifier learns from them. A library
  with few named files gets few kinds from the sound; the user can set kinds by hand, which
  then teach the map too. A sound unlike every learned kind is left Other rather than given the
  nearest one.
- Version 2 caches replace version 1 caches: every library is analysed once more after the
  update.
- t-SNE keeps neighbourhoods but not global distances; the spec says so, and "similar" uses
  feature distances, never map distances.

## Alternatives considered

- **UMAP instead of t-SNE:** keeps more of the global structure and is faster on large
  scopes, but the Rust implementations are young (`umap-rs`, `annembed`) or need a GPU stack
  (`fast-umap`). Revisit when one matures; the layout step is behind one function.
- **A learned embedding** (a neural network over spectrograms, or a pretrained audio model run
  with `tract`): better kinds and similarity, but a model to ship and a training set whose
  licence must allow it. Kept for a later version; openly licensed data (CC-BY or CC0) only.
- **Essentia** for features: complete and tested, but AGPL and C++, and its pretrained models are
  under non-commercial terms. **bliss-audio**: Rust and GPL-3.0-only, but made for whole songs and
  would fix the program's licence to GPL-3.0-only.
- **Self-organising maps** for the grid: a grid by construction, but slower and with layouts
  that depend on training order.
