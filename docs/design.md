# duflow — sistem akış grafı: format, CLI, UI tasarımı

Tarih: 2026-09-06
Durum: taslak, tartışma sonucu. Ayrı repo'da yaşayacak genel bir araç; ilk müşteri Duploy.
Bu doküman ayrı repo açılınca oraya taşınır, burada referans kalır.

Mock (Walk görünümü): https://claude.ai/code/artifact/49a98159-97a0-4e52-889a-693dba6e9f7b

## 1. Amaç

Bir yazılım sisteminin A'dan Z'ye tüm akışını (durumlar, tetikler, endpoint'ler, kontroller,
olası sonuçlar, değişkenler) git'te versiyonlanan metin dosyalarında tutmak; bu grafı hem AI
(token-verimli CLI sorguları, deterministik düzenleme) hem insan (etkileşimli UI, simülasyon,
replay, diff) için sorgulanabilir kılmak.

Dosya koddan türetilmez. Kodla birlikte yaşayan, elle (insan ya da AI) yazılan bir **yansı**dır.
Aynı PR'da kod ve `flows/` birlikte değişir. Drift riski lint ile kısmen kapatılır (bkz. §5),
kod doğrulaması (OpenAPI/route cross-check) v2 eklentisi olarak açık bırakıldı.

## 2. Kapalı kararlar (tartışma özeti)

| # | Karar |
|---|---|
| 1 | Genel araç, domain-bağımsız çekirdek. Duploy ilk müşteri. |
| 2 | Kaynak = repo içindeki `flows/` dizini. Koddan türetme yok, elle yazılan yansı. |
| 3 | Yalnız durum değiştiren olaylar girer; kozmetik (spinner vb.) girmez. Kontrol edilmesi gereken şeyler `check` olarak açıkça yazılır. |
| 4 | Node'lar başıboş olamaz: her node en az bir `root`'tan erişilebilir. "Akış" isimli bir varlık değil, root'tan hedefe giden yolların kümesi. |
| 5 | Değişkenler tipli ve tanımlı (`var`); guard ifadeleri string, yorumlanmaz, içindeki isimler lint'lenir. Her değişkenin yazıldığı yer (`sets`) ya da dış kaynağı (`source`) bilinir. |
| 6 | Aktör ayrı kavram değil; `role` bir `var`. |
| 7 | AI arayüzü: `brief` komutu hazır özet üretir, `--json` ham sorgu. |
| 8 | ID prefix'i = dosya yolu (zorunlu). `deploy.rolling.*` → `flows/deploy/rolling.kdl`. |
| 9 | Hiyerarşi ID'deki noktadan türetilir; ekstra gruplama sözdizimi yok. |
| 10 | UI: soldan sağa akan **Walk** görünümü ana görünüm; simülasyon ve replay aynı görünüm. Diff ve simülasyon v1'de. Read-only. |
| 11 | Hazır graph kütüphanesi yok (Cytoscape/elk çirkin). Tam graph asla çizilmez; her an ≤ ~20 kart, layout trivial, kendimiz render ederiz. |

## 3. Model

### 3.1 Kavramlar

| Kavram | Rol |
|---|---|
| `state` | Sistemin bulunabildiği durum. `layer` attr. |
| `action` | Tetik: click, submit, prompt cevabı, CLI komutu. `calls` ile `call` node'larına bağlanır. |
| `call` | Endpoint / RPC / iç işlem. `check` listesi + `returns` çıkışları. |
| `event` | Async olay: SSE, webhook, timer, docker event. `state`'ler `on` ile dinler. |
| `check` | İsimli guard. Başarısız olunca gidilen hedef. Birden çok `call`'da yeniden kullanılır. |
| `var` | Tipli değişken. `sets` ile yazılır, `when`/`check` ile okunur; ya da `source` ile dış kaynaklı. |
| `root` | Giriş noktası işareti (login, webhook, scheduler tick). Reachability başlangıcı. |
| `view` | Kaydedilmiş sorgu (bookmark). Veri değil, ayrı dosyada. |

### 3.2 ID ve hiyerarşi

- ID: `[a-z0-9_]+(\.[a-z0-9_]+)*`. Örnek `deploy.rolling.wait_healthy`.
- Nokta = hiyerarşi. `deploy.rolling` bir grup, alt ID'lerine "girmek" o gruba erişmek demektir.
  UI gruplama ve kalabalık aday katlaması buradan gelir.
- İlk segment = dosya dizini, ikinci segment = dosya adı: `deploy.rolling.x` → `flows/deploy/rolling.kdl`.
  Tek segmentli ID'ler `flows/<ad>.kdl`. `rename` komutu dosya taşımayı da yapar.
- Referanslar tam ID ile. Kısaltma yok.

### 3.3 Katman

`layer` attr'ı: `ui | api | domain` çekirdekte tanımlı; proje `flow.kdl`'de ek katman
tanımlayabilir. UI'daki katman toggle'ı bu attr'ı filtreler.

### 3.4 Kenarlar

Kenar node'un içinde inline yazılır, ayrı varlık değil:

| Sözdizimi | Anlam |
|---|---|
| `-> "x"` | koşulsuz geçiş |
| `-> "x" when="expr"` | guard'lı geçiş |
| `on "event.id" -> "x" [when=]` | olay dinleme |
| `calls "call.id"` | action → call |
| `returns 202 -> "x"` / `returns 409 code="..." -> "x"` | call çıkışları |
| `check "name" fail=409 code="..." [-> "x"]` | call ön kontrolü; `->` yoksa fail hedefi check tanımından |
| `sets "var.id" "+1"` / `sets "var.id" "0"` | değişken yazımı; değer string, yorumlanmaz |

Aynı hedefe giden birden çok kenar UI'da tek kartta birleşir, etiketler yığılır.

## 4. Format (KDL)

KDL seçildi: yorum destekli, format-preserving edit (`kdl-rs`), graph için TOML'dan okunur,
YAML'dan güvenli, AI'ın hatasız üretmesi kolay. Bir cheatsheet `docs/format.md`'de.

```kdl
// flows/deploy/rolling.kdl
state "deploy.rolling.wait_healthy" layer="domain" {
  desc "Yeni instance healthy olana kadar bekle"
  on "instance.healthy" -> "deploy.rolling.route_switch"
  on "instance.failed"  -> "deploy.rolling.retry"
  on "timeout"          -> "deploy.rolling.retry"
}

state "deploy.rolling.retry" layer="domain" {
  desc "Deneme sayacı artar"
  sets "deploy.attempts" "+1"
  -> "deploy.rolling.create_instance" when="deploy.attempts < 3"
  -> "deploy.rolling.rollback"        when="deploy.attempts >= 3"
}

// flows/api/deploys.kdl
call "api.deploys.create" layer="api" method="POST" path="/api/projects/{id}/deploys" {
  check "perm:deploy.trigger"  fail=404
  check "service_not_frozen"   fail=409 code="service_frozen"   -> "ui.toast.frozen"
  check "has_success_build"    fail=422 code="no_build"         -> "ui.toast.no_build"
  returns 202 -> "deploy.queued"
}

// flows/ui/project.kdl
action "ui.project.deploys.submit" layer="ui" kind="submit" {
  desc "Servis seçilir, Deploy'a basılır"
  calls "api.deploys.create"
}

// flows/vars.kdl
var "deploy.attempts"  type="int"
var "service.replicas" type="int"  source="config:duploy.toml#services.*.replicas"
var "role"             type="enum" source="session" values="platform_admin org_admin member"

// flows/checks.kdl
check "perm:deploy.trigger" desc="Projede deploy.trigger izni" reads="role"

// flows/roots.kdl
root "ui.login"
root "webhook.github.push"
root "scheduler.tick"

// flows/views.kdl  (veri değil)
view "deploy_from_ui" from="ui.login" to="deploy.done"
```

Kurallar:
- Node tanımı tek yerde; başka dosyalar yalnız ID ile referans verir.
- `desc` tek satır, insan dili (projenin dilinde). Uzun açıklama `doc` çocuk node'u.
- `file:line` bilgisi dosyadan gelir, yazılmaz.
- Guard ifadesi: `var` isimleri, sayı/string literal, `== != < <= > >= && || !`. Tokenize edilir,
  yorumlanmaz.

## 5. Lint / validate

| Kural | Seviye |
|---|---|
| Kopuk referans (olmayan ID) | hata |
| Aynı ID iki tanım | hata |
| ID ↔ dosya yolu uyumsuz | hata |
| Root'lardan erişilemeyen node | hata |
| `call` çıkışsız (`returns` yok) | hata |
| Guard'da tanımsız `var` | hata |
| Okunan ama hiç `sets`/`source` olmayan `var` | hata |
| Yazılan ama hiç okunmayan `var` | uyarı |
| Çok çıkışlı `state`'te guard'sız birden fazla koşulsuz `->` | uyarı (belirsizlik) |
| `check` tanımı var, hiç kullanılmıyor | uyarı |
| `desc` eksik | uyarı |

CI: `duflow validate` sıfır hata ile geçmeli.

## 6. CLI

Rust, `clap`, `kdl-rs`, `petgraph`. Tüm komutlar `--json` alır. Exit code: 0 ok, 1 lint hata, 2 kullanım.

| Komut | İş |
|---|---|
| `duflow validate` | §5 |
| `duflow brief <id> [--depth 1]` | AI için tek parça markdown: nasıl gelinir (root'tan en kısa 1-2 yol), okuduğu/yazdığı var'lar, dinlediği event'ler, çıkışları, check'leri, dosya:satır. AI'ın ilk çağrısı budur. |
| `duflow prereq <id>` | Geriye BFS: bu node'a gelmek için geçilmesi gereken check'ler ve sağlanması gereken guard'lar, zincir halinde. |
| `duflow path <a> <b> [--all --max 5]` | İki node arası yollar. |
| `duflow reach <id>` | İleriye erişilebilir küme. |
| `duflow var <id>` | Kim yazıyor, kim okuyor. |
| `duflow search <q>` | ID/desc/check/var üstünde fuzzy. |
| `duflow add <kind> <id> [attr...]` | Dosyaya yazar, yorumları korur. |
| `duflow edit <id> <op>...` | Örn. `--set desc="..."`, `--add-edge "-> x when=..."`, `--rm-edge ...`. |
| `duflow rename <old> <new>` | Tüm referanslar + dosya taşıma. |
| `duflow rm <id>` | Referans varsa reddeder (`--force` ile referansları da siler). |
| `duflow apply -` | stdin'den JSON işlem listesi; AI toplu düzenlemesi için. |
| `duflow diff <rev1> [<rev2>]` | Git rev'leri arası graph diff (eklenen/silinen/değişen node ve kenar; ID bazlı). |
| `duflow ui build [-o dist]` | Statik site: tek `index.html`, graph JSON gömülü. |
| `duflow ui serve` | watch + reload. `?diff=a..b` ile diff overlay. |
| `duflow fmt` | Kanonik biçim. |

Token verimliliği: `brief` çıktısı derinlik 1'de ~30-60 satır hedefler. AI dosyayı okumaz,
brief alır; derinleşmek için `prereq`/`path`.

## 7. UI

Vue 3 + TS, custom SVG/DOM, kütüphanesiz layout. Tek statik `index.html`. Read-only.
Koyu tema birincil, açık tema tam desteklenir. Tipografi: sans gövde + mono ID.

### 7.1 Walk (ana görünüm)

Soldan sağa dört sütun: **geçmiş (1 adım, soluk)** · **şimdi (büyük kart)** · **adaylar (sönük, dikey yığın)** · **ufuk (hover'lanan adayın devamı, daha sönük)**.

- Aday karta hover → ufukta devamı belirir (en fazla 5 + "+n çıkış daha"); hover yalnız ufuk
  sütununu ve vurguyu günceller, sahne yeniden kurulmaz.
- Tıkla → seçilen kart ortaya kayar, eski "şimdi" sola küçülür, seçilmeyen adaylar erir,
  yeni adaylar sağdan girer. FLIP; yalnız izinli sütun geçişleri kayar
  (ufuk→aday, aday→şimdi, şimdi→geçmiş ve geri sararken tersi). Aynı ID başka bir sütunda
  yeniden belirirse kaymaz, yeni girer.
- Kenar etiketleri (`202`, `on instance.healthy`, `when attempts >= 3`) kartta değil, tel
  katmanında (SVG text, arka plan stroke'lu). Guard kesikli tel, fail kırmızı.
- Geçmiş karta tıkla → geri sar. `←` geri.
- Şimdi kartı: kind, layer, ID, desc, check/sets/file chip'leri. Dosya chip'i editöre link.
- Sağ panel: değişken tablosu (kaynak + değer), yürüyüş izi.

### 7.2 Simülasyon = Walk

Ayrı mod yok. Tıklayarak yürümek simülasyondur. `sets` olan adıma girince değişken satırı
yanıp söner, `+1`/`=0` gibi basit ifadeler uygulanır, geri kalanı "değişti" olarak işaretlenir.
Guard'lar UI'da **best-effort** değerlendirilir (`ui/src/expr.ts`: değişken, sayı, string,
karşılaştırma, `&& || !`, parantez): tüm değişkenler biliniyorsa sağlanan dal yeşil/kalın,
sağlanmayan soluk (yine tıklanabilir, "ya şöyle olsaydı"); bilinmeyen değişken varsa nötr.
Yan panelde değişken değeri elle değiştirilebilir. CLI/lint hâlâ yorumlamaz.

### 7.3 Replay

`path`/`prereq` sonucu bir node listesidir; UI aynı animasyonu otomatik oynatır: önce adayı
hover'lar (ufuk belirir), sonra geçer. Hız, duraklat, elle devam. Kalabalık aday grubunda
hedef katlıysa grup otomatik açılır ve karta scroll edilir.

### 7.4 Kalabalık adaylar

≤ 6 çıkış: düz liste. Üstü: hedef ID'nin ilk iki segmentine göre grup; hata dalları
(`fail`) ayrı grup en altta. Grup başına 2 kart + "+n daha"; başlık aç/kapa; üstte süzgeç
(yazınca tüm gruplar açılır). Aday sütunu kendi içinde scroll; scroll'da teller yeniden çizilir.

### 7.5 Katman toggle

Kapalıyken `call` node'ları atlanır; hedef kartta `via POST /api/... · 3 check` rozeti.
Açınca `call` araya kart olarak girer. Toggle tam render tetikler.

### 7.6 Arama (⌘K)

Komut paleti: ID/desc/check/var üstünde fuzzy, eşleşen harfler vurgulu, türe göre gruplu,
ok tuşları + Enter. Seçince o node ortada, root'tan en kısa yol geçmiş olarak dolu.
Check → ait olduğu call'a, var → onu `sets` eden ilk node'a atlar.

### 7.7 Map

Yalnız grup seviyesi: ilk segment kartları (`auth`, `deploy`, `db`...), aralarında kenar
sayısına göre kalınlaşan oklar. Karta tıkla → alt gruplar; node seviyesine inince Walk
(o grubun ilk root'tan erişilen node'undan). Az kart olduğu için basit katmanlı layout,
kendimiz yazarız.

### 7.8 Overlay'ler

- **Diff** (`?diff=a..b`): eklenen yeşil kenar, silinen soluk kırmızı + "artık yok"
  etiketiyle hâlâ aday olarak görünür, değişen sarı halka. Map'te grup kartında `+3 −1 ~2`.
- **Rol filtresi**: guard'ında `role` geçen kenarlar seçili role göre kesilir, erişilemeyen
  adaylar solar.

### 7.9 Vue uygulama notları

- Hover yerel `ref`; ufuk sütunu ona bağlı ayrı component. Hover'ı store'a koyma.
- Kart `key` = `id@col`; izinli geçişte eski key taşınarak `<TransitionGroup>` FLIP'i kullanılır.
- Teller ve etiketler tek SVG katmanında; animasyon bitince (transitionend) çizilir,
  giriş animasyonu sırasında değil.
- Scroll container (aday sütunu) dışına taşan hiçbir şey kartta olmasın (kırpılır).
- `prefers-reduced-motion`'da geçişler anlık.

## 8. Mimari

```
duflow/                      (ayrı repo)
├── crates/duflow-core       # model, KDL parse/write-back, graph, lint, sorgular, diff
├── crates/duflow-cli        # clap; ui dist'i include_bytes ile gömülü
├── ui/                    # Vue 3 + Vite; build → crates/duflow-cli/ui-dist
└── docs/format.md         # KDL cheatsheet
```

- `duflow-core` kütüphane; ileride LSP/editör eklentisi buradan.
- UI graph JSON'u `duflow-core`'un ürettiği tek şema (`schema.json`, versiyonlu).

## 9. Duploy entegrasyonu

- `duploy/flows/` dizini; `flow.kdl` proje ayarı (katmanlar, dil).
- CI: `duflow validate`.
- AI iş akışı: spec-first ("şu akışı flows'a ekle" → `duflow brief` → implement) ya da
  code-first (kod → "flows'u güncelle" → `duflow apply`). AI dosyayı elle düzenlemez.
- İlk içerik: auth (login/refresh/device), proje/config, build, deploy (rolling/rollback),
  managed DB + yedek/restore, cron, volume. Var: `role`, `deploy.attempts`,
  `service.replicas`, `service.frozen`, `service.has_volume`.

## 10. Açık sorular / v2

- Kod cross-check eklentisi: `call.path` ↔ axum router, `check` ↔ `authz` izin adları.
- Cross-repo (frontend/backend ayrı): `flows/` paylaşımı ya da `import`.
- Guard yorumlama (mini expression engine) ve gerçek simülasyon.
- UI'da düzenleme (bilinçli kapalı; dosya:satır linki yeter).
- Araç adı: `flow` çalışma adı, çakışma kontrolü yapılacak.
