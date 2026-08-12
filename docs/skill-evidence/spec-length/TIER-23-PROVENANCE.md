# Tier-2 and tier-3 provenance — the second attempt's relevance and escalation passes

**What this file is.** The SHA-256 of every prompt as dispatched and every reply as read, for
`PROTOCOL-2.md` item 8a's tier-2 pass and item 8b's tier-3 pass. It is the tier-2/3 equivalent of
`retention-3/parts/prompt-hashes.txt` and `parts/assemble-log.txt`, and it exists for the reason
`PROTOCOL-3.md` §5 gives for hashing tier-1's shards: `SCORING-2-NOTES.md` §9.6 found one shard
byte-identical to its own earlier attempt and could not tell a re-emission from a cached completion.
**An attempt is a dispatch, not an independent sample**, and an unhashed reply cannot even raise the
question.

**Why it is a file of its own rather than a `prompt-hashes.txt` inside `adjudication-2/`.** Both
record directories are read by `read_json_records`, which refuses any entry that is not an immediate
`*.json` child — a subdirectory or a `.txt` is a hard error rather than a skipped file, precisely so
that a record no walk sees cannot exist. Dropping provenance in beside the records would break that
guarantee, so it lives here and `RESULTS-2.md` points at it.

**These are not repository paths.** The prompts and the raw replies were written outside the
repository, in the scoring session's scratchpad, exactly as the tier-1 pass wrote its prompts
outside it: a scorer or adjudicator that could list its siblings is not blind. The scratchpad is
disposable and is **not** part of the record; these hashes are what survives it. The prompts are
reproducible from the committed builders — `tools/build-tier2-prompts.py` and
`tools/build-tier3-prompts.py` — against the committed `retention-3/`, `adjudication-2/`, the frozen
ledgers and `generated-2/`.

---

## Tier 2 — item 8a, 18 dispatches, one per generation

**Prompt SHA-256, as dispatched.** 18 prompts, one per generation; every generation's sample was
non-empty, so item 8a's empty-sample case did not arise.

```
01079f0b0dd07aa4afc8d96e99c0a1f06ae333bde0dcd961bd79be8b3a2afdb7  031cc4
d226ef99424b593934283532bb58ad52ead6a5a80b22bbe11b0a78c70b54c46e  054872
567b38e7995b6ff64f7eff37bb5e9091774ade6b3f1414cb2edcc6b39bc1a39f  08ae18
d526c68de74fe3d48f7dcbb6b2fb9714b234c0ae3ee5e13f0faf43a980a666de  26d7a2
dc70e050a094aa05e57fea8b8a5e4820bc1fed82056b04c7c973d7b4e34b9c19  2c4295
94a73226afd50080003376872962d5300721b09696c964bbc89c730774d2e1e3  2d2629
4adc0ce481d3017096795a344ab1250efab116a181797b21741c9c2b7800762d  47173f
dfee78e0c81486a6b1674cbe17fec031bc040bb5a3fd697113b1c3ce274f816c  48527b
1d21452a148507ef86286cb8ae183eae69ba508685c98cd948effc66f1335158  66530f
716169734cd41170e1aed46afec07280a65088ca29b5df02159c2fd42b90f6b7  6e7393
363e62831a1bae73bffe03a2556949fff55c683ceafff4402c602bb6f777e75b  a9fcf9
cb7105992bb41b58eadc1c8a6c038ea87126a56720a9f38613b745f66c6a7e1e  b2b8cf
0822e201f69257fb1a6b6ee93e2f94e09bbef539fc4e908ffed8c1da459f827a  b49ff1
189f4e510d135ba05aeed396e1ae9e2b4187a66cf40a265e63d618b575b87448  e085f2
7dee0c5be821f32cfff27a0610f38d8f434c576efc4ac361732a44df4c1c87a0  e790f5
fcc1083180cfd0f4d145b3548c341697f14680ba7af5d92dfd267452e9f8bb41  fd230c
ab40197e9313a3ccd28361396740c67d813bb6aa2143263a5a297033ae4ef2c5  fd2c24
509fbb8e46ff2e96f54436095963314a295df261d861c7f322424fd3c39b4414  fe4059
```

**Reply SHA-256, as read, with what each carried.** `false` names the rows the adjudicator answered
`establishes: false`; those rows are tier 3's input.

```
031cc4  fb8fd449720f90c664171403043811f5479415247da2be46923ed8dc344f4f11  10 pair(s), 2 false: ['tui-dc-picker-12', 'tui-dc-picker-22']
054872  897df48130744bd1808b144ba388b6fa3616de478bfe2177b447b4ddc77f94e7  15 pair(s), 2 false: ['tiered-review-43', 'tiered-review-78']
08ae18  8981b7130952a06481965fa8b2f9922c00b7b6ad4e0198bea82b1f3fc5969f68  10 pair(s), 3 false: ['tui-dc-picker-11', 'tui-dc-picker-21', 'tui-dc-picker-36']
26d7a2  87fda46bb61262ab799ec279c585ba11b3f1b32641fb36b41449083bc882456b  10 pair(s), 4 false: ['tui-dc-picker-09', 'tui-dc-picker-24', 'tui-dc-picker-34', 'tui-dc-picker-55']
2c4295  3aef01f91764038ab5bdab90c921744c7109aa81edf4c845b8e247557082ac48  16 pair(s), 4 false: ['tiered-review-22', 'tiered-review-48', 'tiered-review-73', 'tiered-review-78']
2d2629  91ddbb79a1f49e08094bdb2eb21b71685d93f826a90e94103651ffd58b037a58  14 pair(s), 6 false: ['tiered-review-06', 'tiered-review-21', 'tiered-review-29', 'tiered-review-36', 'tiered-review-52', 'tiered-review-57']
47173f  b65a298cbfd7da370912c65c2fb87e72164e43e1b3f2d2b802904a099e6afea2  11 pair(s), 2 false: ['tui-dc-picker-35', 'tui-dc-picker-55']
48527b  32a50a0665b38818a384803e55cecc4b1e46aa7ca0c25623cd37f5af65dc42e8  15 pair(s), 4 false: ['tiered-review-13', 'tiered-review-24', 'tiered-review-29', 'tiered-review-34']
66530f  e2e1d3516e607710f6b3200a1f08f8efb4a1ecf3b1127c16be15fe172ffd945c  18 pair(s), 1 false: ['skill-stickiness-55']
6e7393  b4666f88723858a98d445a221980b1473e6959cd10106de33dc2b5799f833bc0  18 pair(s), 4 false: ['skill-stickiness-26', 'skill-stickiness-36', 'skill-stickiness-46', 'skill-stickiness-56']
a9fcf9  c422a3ba898e744f918dba8d46c1521a147c42f8f1cb11f2de2cc8a9b29cd6f1  11 pair(s), 3 false: ['tui-dc-picker-10', 'tui-dc-picker-20', 'tui-dc-picker-45']
b2b8cf  bc0e638e016b956206230645053ef3ce96e1d4402fc39bef2f7a1a3f3f79ef98  10 pair(s), 0 false: -
b49ff1  74e4b6a032322343ba5a9300bde8c982bada5e677ecb9c67c673e5faf6221ef3  16 pair(s), 3 false: ['tiered-review-25', 'tiered-review-30', 'tiered-review-35']
e085f2  e4be2526cf5a4bea81a648270914edd8026e8b2b74a30abd12f9f80dd7e29f78  17 pair(s), 2 false: ['skill-stickiness-67', 'skill-stickiness-87']
e790f5  fca9402ed55cc7cd9ed01e8f73d8f12b535c7e3002092d554a38caba60b57770  18 pair(s), 3 false: ['skill-stickiness-15', 'skill-stickiness-55', 'skill-stickiness-81']
fd230c  664579cee76284bc655cc4fabb6cae07acb1a6534e9366ee827de6bdb862ec56  16 pair(s), 3 false: ['tiered-review-05', 'tiered-review-25', 'tiered-review-30']
fd2c24  17d218f64c3fc35187e1adea6b18bd537168734ca19f7e1861e8754663b64239  18 pair(s), 1 false: ['skill-stickiness-75']
fe4059  c40afc1d44a9e1b8414c440f31b1b76778ef2ecc9924137d5f9c24cb9182df3a  17 pair(s), 4 false: ['skill-stickiness-20', 'skill-stickiness-55', 'skill-stickiness-66', 'skill-stickiness-87']
```

## Tier 3 — item 8b, 18 dispatches over 17 prompts, one prompt per generation carrying a flagged row

**18 dispatches, 17 prompts, 17 records.** `fe4059`'s prompt was dispatched twice: its first
dispatch wrote no file and was re-run under `R6` against the byte-identical prompt (see the last
section of this file). A dispatch that wrote no file still ran, so it is counted here.

`b2b8cf` carried no flagged row — no tier-2 `establishes: false` and no `PROTOCOL-3.md` §3 class-B
problem — so it has **no prompt and no `escalation-2/b2b8cf.json`**. Item 8b makes that absence the
record, and `spec_length_2_final_disposition_is_the_recorded_join` reads it that way.

**Prompt SHA-256, as dispatched.**

```
b86611e16ea0c02d6225e41a12f1521386dd52ec2a92a93f1b0cdb455bcac3e8  031cc4
b5cb89b82625c66ed117d8ed296acd7b1ea62f81d132023184a40b48c3cec9b6  054872
0d8c1f415c41e03adba256cdc597db4ff949a3bbc4b31319c7136c9684d37100  08ae18
d9a3c45c4b9fea3886413e2823389d99b154bd888084b0c5afd1aae0b71659c8  26d7a2
170c5c72471f584011cb3ccc96d57aec5e186e9c46dfa86bd3e04e7e73df98cc  2c4295
e3e07d3f964aad733dc17a7bb15ee35ba2a8bd14a277c1d57d603ba40503bd5c  2d2629
ea291928b920e5217a5a036a923d570b45ee0514d3b91e0e6e9df1434ac125ce  47173f
2469296969166e24f67fe202a7d2ff0ef3e945a8b1fc6a545cd6a4c709b3046b  48527b
d3687b3e27d3af31608600c06276a3e37986897bae473d5cd67f556f29eca5ca  66530f
ea0be25df583c7d3c8aa3ee5414f2c5dc13821f2b6fd7b05df8400ae14d13c9a  6e7393
7ab192ffd33f52da66af358e76bb1c4076115969dbf10e24c5e20599a12997fd  a9fcf9
097a76b92e78ac2d7363b3b4562194a72bde3fd9d2999ad6e36a5db511ea5a56  b49ff1
5dfcf29dd61dd00ae65002723d996f3c6f4cedc2fe4f6cb279edc923bd6edcf4  e085f2
d2e4ea63f87f247c2b351067e7855effb2304b3656f016e564647d171f60303c  e790f5
cc4804e225691972d28ae67997e40fc78495ba0a8ee3ef31835c9d227405290c  fd230c
1f928aaf36dd5f1887cabc9c10b51bafe08ac37ad7c9fd14c422feddfe330c24  fd2c24
4785b67ab54ba40edc4d0061d737adf19fae3c711a7b868f709386dd1b5b942a  fe4059
```

**Reply SHA-256, as read, with what each carried.**

```
031cc4  61bf8d3295b03452d61f6246c37337c94b162b8db7d3170af32e63372988f6f9  3 escalated, 0 answered present:false: -
054872  97f3863d12e7d19c01634fd3698fb07fb046b7376609eee63e3d9a1419a1b5a6  8 escalated, 0 answered present:false: -
08ae18  37e1eb970a0acca54b68059500bbbfe823ed2db35da5889f4735cf2a4768f0b6  3 escalated, 0 answered present:false: -
26d7a2  47e5c2e287bac1725e7ee8e3455e6067813fe5fba0f473de59cc41b3b8ea5d7f  9 escalated, 1 answered present:false: ['tui-dc-picker-24']
2c4295  b2793366c74afd155012aa47683bdd05314f4a6622400e7d7644cf1ac5b55e66  6 escalated, 1 answered present:false: ['tiered-review-22']
2d2629  3e7c5d50b24fedd8c7093a9d3e4ac7d3e77565ee8921de2bd4e7dbfe94ea7a37  23 escalated, 0 answered present:false: -
47173f  ccce466c45e0b67cdc799c621de43271109c6cb3fd2170e41db9f2aecfce756c  2 escalated, 0 answered present:false: -
48527b  4c29b82e7419fc74603b2283dafaa1141c2057776181ead080a77fcb28331227  13 escalated, 0 answered present:false: -
66530f  57c1a05e87c066942806f59f0d17a4ae10167134648ab644f45b2378754b1b9e  3 escalated, 0 answered present:false: -
6e7393  d3e1da9ca6002038299dc0735a68770820ad9463ab7389fb9725f686fb1b7665  4 escalated, 0 answered present:false: -
a9fcf9  a72d0b0c2013b742ec19feec21cfd6a42ecfea7550a5843000ea8d2ce62abb19  3 escalated, 0 answered present:false: -
b49ff1  24c67510df2f57cb2a9142bf18785948e5ab85b8fe53427d9e55bfd5321092a6  3 escalated, 0 answered present:false: -
e085f2  5ebb9a53b4619d5f927ebd99e817fcadc255caaaa2c2283740ac693d446d3abe  2 escalated, 0 answered present:false: -
e790f5  06417981107c5afdca8d4a93b5a9a0a78f64e56e3677581f7c1217121bf72f68  3 escalated, 0 answered present:false: -
fd230c  ffe40fc8b3ee6e13aafc17f46479a2677097b7cc044a3a757e51e4e6a1ea741e  3 escalated, 0 answered present:false: -
fd2c24  20e69966906e87de40b38043621a4a31ff79c3114f2304b802b9e3b89a455523  3 escalated, 0 answered present:false: -
fe4059  9b65b2f45ceccb7e43ffc1bf63ad177906bae921602a28e001a06d93f80e3ff4  7 escalated, 0 answered present:false: -
```

---

## The one `R6` re-run in these two passes

**`fe4059`, tier 3, first dispatch: the probe wrote no file.** It read its prompt, formed an answer
and returned it as its reply text rather than writing it to the path it was given. That is `R6`'s
first trigger verbatim — *the probe wrote no file* — and the remedy is a re-run of that dispatch
whole.

**The returned text was not harvested, and that is the point worth recording.** It would have been
one copy-paste to lift the answer out of the transcript, and it would have produced a record whose
provenance is a channel no rule in this protocol sanctions. `R6` says re-run; the dispatch was
re-run against the byte-identical prompt (`4785b67ab54ba40e…`, the `fe4059` row above) and the
re-run's reply is what `escalation-2/fe4059.json` holds.

**It is not a re-run in `SCORING-3-NOTES.md` §8a's sense and does not count against that bound.**
§8a bounds *tier-1* dispatches against a generation that already has a verdict or a superseded
shard, and it excludes `R6` re-runs explicitly. This is a tier-3 dispatch that produced no record at
all, re-run under the trigger written for exactly that case.
