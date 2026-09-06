# CLAUDE.md

## duflow nedir?

Bir yazılım sisteminin A'dan Z'ye akış grafını (durumlar, tetikler, endpoint'ler, kontroller,
olası sonuçlar, değişkenler) git'te versiyonlanan **KDL** dosyalarında tutan genel bir araç.
Graf hem AI için (token-verimli CLI sorguları, deterministik write-back) hem insan için
(tek dosyalık etkileşimli UI: Walk, simülasyon, replay, Map, diff overlay) sorgulanır.
Dosyalar koddan türetilmez; kodla birlikte yaşayan, elle (insan ya da AI) yazılan bir yansıdır.
İlk müşteri Duploy (`~/dev/duploy/flows/`, orada gitignore'lu deneme alanı).

Tasarım kararları ve gerekçeler: `docs/design.md` (spec). Bu dosya yalnız çalışma rehberi.

## Yapı

```
duflow/
├── crates/duflow-core   # model, KDL parse (kdl 6), Graph + indeks, lint, query (brief/prereq/path/search), diff, edit (format-preserving write-back), export (UI JSON şeması v1)
├── crates/duflow-cli    # `duflow` binary'si (clap); ui.rs: ui-dist/index.html'i include_str ile gömer, `/*DUFLOW_DATA*/null/*END*/` yerine JSON basar
│   └── ui-dist/index.html   # `bun run build` çıktısı (vite-plugin-singlefile). Derlenmiş; commit'lenir ki cargo build UI'sız da çalışsın
├── ui/                  # Vue 3 + Vite + TS. src/graph.ts (veri + saf sorgular), src/walk.ts (store), components/{TopBar,Walk,Card,SidePanel,MapView}.vue
│   └── public/duflow.json   # dev'de yüklenen örnek export (`duflow export > ui/public/duflow.json`)
├── docs/design.md       # spec
└── .claude/skills/duflow/SKILL.md   # AI için kullanım rehberi (format + CLI), İngilizce
```

## Komutlar

```bash
just build         # UI (bun) + cargo build --release
just test          # cargo test --workspace
just ui-dev        # Vite dev server (localhost:5173), public/duflow.json'u yükler
just ui-build      # tek dosya UI → crates/duflow-cli/ui-dist/index.html (sonra cargo build gerekir)
just try           # Duploy flows'u ile validate + brief örneği
cargo run -p duflow-cli -- -d ~/dev/duploy/flows validate
```

## Kurallar

- **Kod, CLI çıktıları, UI metinleri ve skill dosyası İngilizce; yorumlar ve docs/ Türkçe.**
- Format değişikliği = `docs/design.md` §3-4 güncellenir, `parse.rs` + `edit.rs` + skill dosyası birlikte değişir.
- `edit.rs` biçim korur: `autoformat()` çağırma (tırnakları siler); string değerler `str_arg/str_prop/set_str` ile yazılır (`value_repr` tırnaklı).
- ID→dosya kuralı: `a.b` → `a.kdl` ya da `a/b.kdl`; `a.b.c…` → `a/b.kdl` ya da `a.kdl` (`model::allowed_files`). `add` var olanı seçer.
- Lint'te "erişilebilirlik" `on` ile dinlenen event'leri de kapsar (`Graph::listened_events`).
- UI Walk = **konveyör**: 5 eşit slot `[gizli-sol][geçmiş][şimdi][adaylar][ufuk/gelen]`, bant tek `translateX` ile kayar; ileri: seçilenin devamı sağ slota statik çizilir → kay → `go()` → bant sıfırlanır (görüntü aynı). Kart bazlı FLIP yok, animasyon sırasında ölçüm/çizim yok. Hover **yerel** (Walk.vue), store'a koyma; teller ve etiketler bant içindeki tek SVG'de; slot (`overflow:auto`) dışına taşan hiçbir şey kartta olmasın.
- Export şeması değişirse `export.rs::SCHEMA_VERSION` ve `ui/src/graph.ts` tipleri birlikte.
- Commit mesajları kısa, Conventional Commits, attribution yok.
- Autocomplete `clap_complete` dynamic (`unstable-dynamic`): `CompleteEnv` main'in başında, ID argümanları `ArgValueCompleter` ile grafı o an yükler (`-d` görülmez; cwd ya da `DUFLOW_DIR`). Kurulum satırı `duflow completions <shell>`.
