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
| `check` | İsimli guard; herhangi bir node içinde kullanılır (call, state, action). Başarısız olunca gidilen hedef ya da node'suz `outcome`. Birden çok yerde yeniden kullanılır. |
| `perm` | İzin tanımı (`scope`, `deny` kodu, varsayılan fail hedefi). Node'da `requires "x"` = `perm:x` check'i. |
| `var` | Tipli değişken. `sets` ile yazılır, `when`/`check` ile okunur; ya da `source` ile dış kaynaklı. |
| `root` | Giriş noktası işareti (login, webhook, scheduler tick). Reachability başlangıcı. `every="30s"` ile periyodik (reconciler, canlılık turu): sahte kenar yerine root. |
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

### 3.3.1 Proje ayarı (`flow.kdl`)

```kdl
project "duploy" {
  layers "ui" "api" "domain"
  watch "dorch/src/api/**" "dorch/src/deploy/**"   // grafı etkileyebilecek kaynak glob'ları (repo köküne göre)
  max_roots 10                                     // üstü `too_many_roots` uyarısı (varsayılan 10)
}
```

`watch` çekirdek sorgularda kullanılmaz; editor hook'ları (§6.1) bu dosyalar değişince `flows/`
güncellenmiş mi diye bakar. Dar tut: geniş glob "gerek yok" demeyi öğretir, sonra gerçek drift kaçar.

### 3.4 Kenarlar

Kenar node'un içinde inline yazılır, ayrı varlık değil:

| Sözdizimi | Anlam |
|---|---|
| `-> "x"` | koşulsuz geçiş |
| `-> "x" when="expr"` | guard'lı geçiş |
| `-> "x" case="pool_exhausted"` | etiketli dal: guard'sız gerçek dallanma (fan-out, "ya/ya da"). `case`, `when` ya da `seq` taşıyan kenar belirsiz sayılmaz |
| `-> "x" seq=1` / `-> "y" seq=2` | sıralı adımlar ("hepsi sırayla"): purge zinciri, tick'in üç taraması. Dallanma değil; aynı node'da tekrar eden `seq` → `duplicate_seq` |
| `-> outcome="toast: hatalı şifre" [when=] [case=]` | hedefsiz `->`: guard'lı/etiketli node'suz son (toast, log). `returns`/`check` outcome'unun düz kenar karşılığı |
| `on "node.id" -> "x" [when=] [case=]` | olay dinleme; hedef `event` ya da herhangi bir node (state'e giriş de olaydır). Yalnız `event`'ler dinlenerek erişilebilir olur |
| `calls "call.id"` | action → call |
| `returns 202 -> "x"` / `returns 200 case="pr" -> "x"` / `returns 409 code="..." -> "x"` | call çıkışları; aynı status birden çok sonuca `case` ile ayrılır |
| `returns 409 code="..." outcome="toast: volume in use"` | node'suz terminal çıkış (toast, log satırı). UI'da yaprak, aday değil |
| `check "name" fail=409 code="..." [-> "x" \| outcome="..."]` | ön kontrol; `->`/`outcome=` yoksa fail sonu check tanımındaki **varsayılan** (`-> "x"` ya da `outcome="..."`), kullanımdaki onu ezer. Kullanım `outcome=` dediyse tanımın hedefine kenar çizilmez |
| `requires "perm.id" [fail=404 code="..."] [-> "x" \| outcome="..."]` | izin; `perm` tanımındaki `deny` fail kodu, `fail_to` varsayılan hedef; kullanım `fail=`/`code=` ile ezer (registry'nin 404 NAME_UNKNOWN dönmesi gibi) |
| `sets "var.id" "+1"` / `sets "retry" "0"` | değişken yazımı; değer string, yorumlanmaz |

Aynı hedefe giden birden çok kenar UI'da tek kartta birleşir, etiketler yığılır.

**Dış girişler (`entry=`).** UI'sı olmayan yönetim/CLI uçları için sahte state açılmaz: `call ... entry="cli"`
(ya da `api`) node'u dış istemcinin doğrudan çağırdığı giriş sayar; erişilebilirlik oradan başlar,
`too_many_roots` sayımına girmez. `root` kullanıcı/sistem girişleri (login, webhook, tick) içindir.

**Olay konvansiyonu.** `event` node'undan `->` çıkmaz (`event_has_transition`): olay yalnız `on` ile
tüketilir. Teslimat altyapısı (emit → SSE → `ui.events.received`) grafta bir kez, kendi zincirinde
modellenir; domain olayından UI'ya doğrudan kenar çizilmez.

**Yerel sayaçlar.** Guard'daki noktasız isim (`retry`, `attempts`) tanımlı bir `var` değilse yereldir:
`var` tanımı istemez, `vars` dosyasına girmez; yalnız aynı kapsamda bir `sets` olmalı
(`local_var_never_set`). Kapsam = ID'nin son noktaya kadarki öneki: `restore.snapshot` ile
`restore.applying` aynı kapsam (`restore`), `deploy.rolling.retry` için `deploy.rolling`. Noktalı isimler ve tanımlı olanlar (`role`) globaldir, `var` ister.
Global `var` yalnız gerçekten dış kaynaklı ya da gruplar arası okunanlar için.

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
call "api.deploys.create" layer="api" method="POST" path="/api/projects/{id}/deploys" src="dorch/src/api/deploys.rs#trigger_deploy" {
  requires "deploy.trigger"
  check "service_not_frozen"   fail=409 code="service_frozen"   -> "ui.toast.frozen"
  check "has_success_build"    fail=422 code="no_build"         outcome="toast: başarılı build yok"
  returns 202 -> "deploy.queued"
  returns 200 case="noop" -> "ui.project.deploys"
}

// flows/network.kdl — reconciler: periyodik root, sahte kenar yok
state "network.liveness" layer="domain" desc="30 sn'de bir node canlılık turu" {
  check "node_reachable" -> "network.node_down"
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
check "service_not_frozen" desc="Servis restore'da kilitli değil" reads="service.frozen" -> "ui.toast.frozen"

// flows/perms.kdl
perm "deploy.trigger" scope="project" deny=404 desc="Projede deploy izni" -> "ui.toast.not_found"

// flows/roots.kdl
root "ui.login"
root "webhook.github.push"
root "network.liveness" every="30s"

// flows/views.kdl  (veri değil)
view "deploy_from_ui" from="ui.login" to="deploy.done"
```

Kurallar:
- Node tanımı tek yerde; başka dosyalar yalnız ID ile referans verir.
- `desc` tek satır, insan dili (projenin dilinde). Uzun açıklama `doc` çocuk node'u.
- `file:line` bilgisi dosyadan gelir, yazılmaz. Kod bağı için `src="path#symbol"` (ya da `path:line`)
  attr'ı: `validate` dosyanın varlığını ve sembol adının dosyada geçtiğini kontrol eder (dil bilmez).
- Tanımların dosyası: noktalı `var` node'larla aynı ID→dosya kuralı (`deploy.attempts` → `deploy.kdl`),
  noktasız `var` → `vars.kdl`; `check` → `checks.kdl`, `perm` → `perms.kdl`, `root` → `roots.kdl`.
  Okurken her dosya kabul edilir; kural yalnız `add`'in nereye yazacağını belirler.
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
| Guard'da tanımsız noktalı `var` (`unknown_var`) | hata |
| Yerel sayaç grupta hiç `sets` edilmiyor (`local_var_never_set`) | hata |
| `requires` edilen `perm` yok (`unknown_perm`) | hata |
| Okunan ama hiç `sets`/`source` olmayan `var` | hata |
| Yazılan ama hiç okunmayan `var` | uyarı |
| Birden fazla `when`'siz, `case`'siz **ve** `seq`'siz `->` (`ambiguous_transition`) | uyarı (belirsizlik) |
| Aynı node'da tekrar eden `case=` / `seq=` (`duplicate_case`, `duplicate_seq`) | uyarı |
| Aynı node'da `seq=` ile `case=` karışık (`mixed_branching`): dal son seq adımının node'una | uyarı |
| `event` node'undan `->`/`calls`/`returns` çıkıyor (`event_has_transition`) | uyarı |
| `on` hedefi hiç yok (`unknown_event`) | uyarı |
| `check`/`perm` tanımı var, hiç kullanılmıyor | uyarı |
| Root'un giren kenarı var (`root_has_incoming`) | uyarı (uydurma root freni) |
| Periyodik olmayan (`every`'siz) root sayısı `max_roots` üstü (`too_many_roots`) | uyarı |
| `src=` dosyası yok / sembol dosyada tam kelime olarak geçmiyor / kod dosyasına sembolsüz-satırsız işaret (`src_missing`, `src_symbol_missing`, `src_symbol_unchecked`) | uyarı |
| `desc` eksik | uyarı |

`validate --prefix deploy` yalnız o namespace'i (ID ya da dosya öneki), `--summary [--depth 2]` kategori ×
namespace tablosunu verir (`--depth 2`: `api.deploys` ile `api.auth` ayrı sütun); paralel çalışan
ajanlar kendi hatalarını kendileri ayıklar.

CI: `duflow validate` sıfır hata ile geçmeli.

## 6. CLI

Rust, `clap`, `kdl-rs`, `petgraph`. Tüm komutlar `--json` alır. Exit code: 0 ok, 1 lint hata, 2 kullanım.

| Komut | İş |
|---|---|
| `duflow validate [--prefix ns ...] [--summary [--depth n]] [--no-src]` | §5 |
| `duflow brief <id> [--depth 1]` | AI için tek parça markdown: nasıl gelinir (root'tan en kısa 1-2 yol), okuduğu/yazdığı var'lar, dinlediği event'ler, çıkışları, check'leri, dosya:satır. AI'ın ilk çağrısı budur. |
| `duflow prereq <id>` | Geriye BFS: bu node'a gelmek için geçilmesi gereken check'ler ve sağlanması gereken guard'lar, zincir halinde. |
| `duflow path <a> <b> [--all --max 5]` | İki node arası yollar. |
| `duflow reach <id>` | İleriye erişilebilir küme. |
| `duflow var <id>` | Kim yazıyor, kim okuyor. |
| `duflow perm [<id>]` | İzinler; tek izin verilirse onu `requires` eden uçlar ("bu uca kim erişir"). |
| `duflow search <q>` | ID/desc/check/var üstünde fuzzy. |
| `duflow ls [--kind k] [--layer l] [--prefix p ...] [--attr k[=v]] [--no-attr k]` | Liste; `--kind call --no-attr method` sahte call avı. |
| `duflow add <kind> <id> [attr...]` | Dosyaya yazar, yorumları korur. `kind`: node türleri + `var check root perm`. |
| `duflow edit <id> <op>...` | `--set k=v`, `--kind call` (tür değişir, çocuklar kalır), `--child '<kdl>'`, `--rm-edge x`, `--rm-child name:arg`. Tanımlar (var/check/root/perm) da `--set` alır. **Silmeler eklemelerden önce uygulanır**: aynı komutta "kenarı sil, `case=` ile geri ekle" olur. |
| `duflow rename <old> <new>` | Tüm referanslar + dosya taşıma. |
| `duflow rm <id> [--force]` | Node ya da tanım. Referans varsa reddeder; `--force` **referansları da siler** (kenarlar, `sets`, check/requires kullanımları, root/view satırı, check/perm tanımındaki `-> x`/`fail_to`, var için `reads` kelimesi). Node'u yeniden yazmak için `rm`+`add` değil `edit --kind`. |
| `duflow apply - [--dry-run] [--continue-on-error]` | stdin'den JSON işlem listesi. Atomik: bir op düşerse hiçbir şey yazılmaz, **tüm** düşen op'lar indeksiyle listelenir; `--dry-run` aynı listeyi verir. |

Yazma kilidi: `Workspace` açılırken dizin başına flock (`$TMPDIR/duflow-lock/<hash>.lock`), commit'e
kadar tutulur; paralel ajanlar birbirini ezmez, sıraya girer.
| `duflow diff <rev1> [<rev2>] [--stat]` | Git rev'leri (ya da flows dizini yolları, gitignore'lu flows için kopya) arası graph diff (eklenen/silinen/değişen node ve kenar; ID bazlı). `--stat`: namespace bazlı sayılar; ajan kaybettiği kenarı görür. |
| `duflow ui build [-o dist]` | Statik site: tek `index.html`, graph JSON gömülü. |
| `duflow ui serve` | watch + reload. `?diff=a..b` ile diff overlay. |
| `duflow fmt` | Kanonik biçim. |

Token verimliliği: `brief` çıktısı derinlik 1'de ~30-60 satır hedefler. AI dosyayı okumaz,
brief alır; derinleşmek için `prereq`/`path`.

### 6.1 Claude Code plugin'i ve hook'lar

Skill tek başına hatırlatmadır; model alışır, es geçer. Plugin (`plugin/`, marketplace bu repo)
skill'in yanına deterministik kapılar koyar, mantık `duflow hook <event>` içinde:

| Hook | İş |
|---|---|
| SessionStart | Baseline: HEAD, kirli dosyaların blob hash'i, `flows/*.kdl` içerik hash'i. Modele proje bağlamı + `watch` listesi. |
| PostToolUse (Edit/Write) | Dosya `watch` glob'unda ise dosya başına bir kez hedefli hatırlatma (ekranda görünmez, modele context). |
| Stop | Oturumda `watch` dosyası değişmiş ama `flows/` değişmemişse ya da **bu oturumda değişen flows dosyalarında** lint hatası varsa `decision: block`; model ya grafı günceller ya tek cümle "etkilenmedi" der. Başka namespace'in hatası bloklamaz (paralel ajanlar). `stop_hook_active` ikinci gelişte geçirir. |
| PreToolUse (`git commit`) | Lint hatası → `deny`. Drift → sadece uyarı (kod ve flows ayrı commit olabilir). |

`flows/` yoksa, `duflow` PATH'te yoksa ya da baseline yoksa hook sessiz; plugin global kurulunca
alakasız repoyu rahatsız etmez. CI'daki `duflow validate` son tampon.

Skill dosyası Claude'a özel değil (agent-skills standardı); Claude dışı ajanlar için binary'ye
gömülü kalır (`duflow skill install`).

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

- **Konveyör:** 5 eşit genişlikte slot; adım = sağ slota yeni içeriği statik çiz → bandı tek `translateX` ile kaydır → bitince state'i ilerlet ve bandı sıfırla. Kart bazlı FLIP terk edildi (mid-animasyon ölçümleri glitch üretiyordu).

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

- **Alt-akış / çağırana dönüş.** "Niyet yaz → sync → bekle" kalıbı 5-6 yerde tekrar ediyor. Paylaşılan
  grup ID ile referanslanabilir ama "çağırana geri dön" (subroutine) grafta yok; parametreli şablon
  düşünülmedi, tekrar kabul edildi.

- Kod cross-check eklentisi: `call.path` ↔ axum router, `check` ↔ `authz` izin adları.
- Cross-repo (frontend/backend ayrı): `flows/` paylaşımı ya da `import`.
- Guard yorumlama (mini expression engine) ve gerçek simülasyon.
- UI'da düzenleme (bilinçli kapalı; dosya:satır linki yeter).
- Araç adı: `flow` çalışma adı, çakışma kontrolü yapılacak.
