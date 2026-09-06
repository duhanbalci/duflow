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
├── crates/duflow-cli    # `duflow` binary'si (clap); ui.rs: ui-dist/index.html'i include_str ile gömer, `/*DUFLOW_DATA*/null/*END*/` yerine JSON basar; hook.rs: Claude Code hook'ları (`duflow hook <event>`)
│   └── ui-dist/index.html   # `bun run build` çıktısı (vite-plugin-singlefile). Derlenmiş; commit'lenir ki cargo build UI'sız da çalışsın
├── ui/                  # Vue 3 + Vite + TS. src/graph.ts (veri + saf sorgular), src/walk.ts (store), components/{TopBar,Walk,Card,SidePanel,MapView}.vue
│   └── public/duflow.json   # dev'de yüklenen örnek export (`duflow export > ui/public/duflow.json`)
├── docs/design.md       # spec
├── install.sh           # curl | sh kurulum; GitHub Release asset'ini indirir
├── .github/workflows/   # ci.yml (test), release.yml (v* tag → 4 hedef binary + GitHub Release)
├── plugin/              # Claude Code plugin'i: .claude-plugin/plugin.json, skills/duflow/SKILL.md, hooks/hooks.json, scripts/hook.sh
│   └── skills/duflow/SKILL.md   # AI için kullanım rehberi (format + CLI), İngilizce; binary'ye include_str ile gömülür (`duflow skill install`, Claude dışı ajanlar için)
├── .claude-plugin/marketplace.json   # `claude plugin marketplace add duhanbalci/duflow` → source ./plugin
└── .claude/skills/duflow → ../../plugin/skills/duflow (symlink; bu repoda çalışırken skill aktif)
```

## Komutlar

```bash
just build         # UI (bun) + cargo build --release
just test          # cargo test --workspace
just ui-dev        # Vite dev server (localhost:5173), public/duflow.json'u yükler
just ui-build      # tek dosya UI → crates/duflow-cli/ui-dist/index.html (sonra cargo build gerekir)
just try           # Duploy flows'u ile validate + brief örneği
cargo run -p duflow-cli -- -d ~/dev/duploy/flows validate
just release 0.2.0 # Cargo.toml bump + tag + push; Actions binary'leri derleyip release açar
```

Repo: github.com/duhanbalci/duflow (public). Release asset adı `duflow-v<ver>-<target>.tar.gz`;
`duflow self-update` (self_update crate, ureq+rustls) ve `install.sh` bu ada bağlı — değiştirme.

## Kurallar

- **Kod, CLI çıktıları, UI metinleri ve skill dosyası İngilizce; yorumlar ve docs/ Türkçe.**
- Format değişikliği = `docs/design.md` §3-4 güncellenir, `parse.rs` + `edit.rs` + skill dosyası birlikte değişir.
- `edit.rs` biçim korur: `autoformat()` çağırma (tırnakları siler); string değerler `str_arg/str_prop/set_str` ile yazılır (`value_repr` tırnaklı).
- ID→dosya kuralı: `a.b` → `a.kdl` ya da `a/b.kdl`; `a.b.c…` → `a/b.kdl` ya da `a.kdl` (`model::allowed_files`). `add` var olanı seçer.
- Lint'te "erişilebilirlik" `on` ile dinlenen event'leri de kapsar (`Graph::listened_events`).
- UI Walk = **konveyör**: 5 eşit slot `[gizli-sol][geçmiş][şimdi][adaylar][ufuk/gelen]`, bant tek `translateX` ile kayar; ileri: seçilenin devamı sağ slota statik çizilir → kay → `go()` → bant sıfırlanır (görüntü aynı). Kart bazlı FLIP yok, animasyon sırasında ölçüm/çizim yok. Hover **yerel** (Walk.vue), store'a koyma; teller ve etiketler bant içindeki tek SVG'de; slot (`overflow:auto`) dışına taşan hiçbir şey kartta olmasın.
- Export şeması değişirse `export.rs::SCHEMA_VERSION` ve `ui/src/graph.ts` tipleri birlikte (şu an 2: `in`, `outcomes`, `every`, `perms`).
- `edit.rs`: `Workspace::open` dizin kilidi alır (`$TMPDIR/duflow-lock/`), düşene kadar tutar. `apply_all` hataları toplar, atomiklik CLI'da. `rm` her türü siler (`--force` referansları da). Tanım dosyaları: noktalı var → ID kuralı, noktasız var → `vars.kdl`, check/perm/root → `checks/perms/roots.kdl`.
- `requires "x"` parse'ta `perm:x` check kullanımına dönüşür; `Graph::reindex` perm'den sentetik `CheckDef` üretir, `fail` kodunu `deny`'dan doldurur.
- Yerel var: guard'daki noktasız, tanımsız isim (`Graph::is_local_var`); lint aynı grupta `sets` arar.
- Commit mesajları kısa, Conventional Commits, attribution yok.
- Hook'lar (`hook.rs`): SessionStart baseline yazar (`$TMPDIR/duflow-hook/<session>.json`: HEAD, kirli dosya hash'leri, flows/ hash'leri), PostToolUse `watch` glob'una uyan dosyada tek seferlik hatırlatma, Stop drift/lint varsa `decision: block`, PreToolUse `git commit` lint hatasında `deny`. Baseline yoksa sessiz. `flows/` gitignore'lu olsa da hash ile izlenir.
- Plugin sürümü `plugin.json` + `marketplace.json`'da; `just release` ikisini de bump'lar.
- Autocomplete `clap_complete` dynamic (`unstable-dynamic`): `CompleteEnv` main'in başında, ID argümanları `ArgValueCompleter` ile grafı o an yükler (`-d` görülmez; cwd ya da `DUFLOW_DIR`). Kurulum satırı `duflow completions <shell>`.
