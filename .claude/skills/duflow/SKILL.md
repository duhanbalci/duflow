---
name: duflow
description: duflow ile sistem akış grafı (flows/*.kdl) okuma, sorgulama ve düzenleme. Kullanıcı bir akışı/adımı/ön koşulu sorduğunda, "flows'a ekle/güncelle" dediğinde, ya da bir özelliği implement etmeden önce bağlam gerekince kullan. Dosyayı elle düzenleme; CLI'dan geç.
---

# duflow

Sistemin akış grafı `flows/` dizininde KDL dosyalarında durur. Kod değil, kodun **yansısı**:
durumlar, tetikler, endpoint'ler, kontroller, olası sonuçlar, değişkenler. Kodla aynı PR'da değişir.

## İş akışı

1. **Bağlam al, dosya okuma.** İlgili node'un özetini iste:
   ```bash
   duflow brief <id>            # nasıl gelinir, gelen/çıkan, check'ler, var'lar, dosya:satır
   duflow prereq <id>           # buraya gelmek için gereken adımlar, geçilmesi gereken check'ler, gereken var'lar
   duflow path <a> <b> [--all]  # iki node arası yol(lar)
   duflow search <metin>        # ID/desc/check/var fuzzy
   duflow var <var.id>          # kim yazıyor, kim okuyor
   duflow ls --prefix deploy    # liste (--kind, --layer)
   ```
   Her komut `--json` alır. `flows/` bulunamazsa `-d <dir>` ver.
2. **Kod yazdıysan grafı güncelle** (code-first) ya da **önce grafı yaz, sonra implement et** (spec-first).
3. **Düzenlemeyi CLI'dan yap**, dosyayı elle açma (biçim ve referans tutarlılığı CLI'da):
   ```bash
   duflow add state deploy.done --layer domain --desc "Deploy success" --child '-> "deploy.status"'
   duflow add call api.x.create --layer api --attr method=POST --attr path=/api/x \
     --child 'check "perm:x.create" fail=404' --child 'returns 202 -> "x.queued"'
   duflow add var x.count --attr type=int         # var/check/root da `add` ile
   duflow edit <id> --set desc="..." --child 'on "ev" -> "x"' --rm-edge <hedef> --rm-child sets:x.count
   duflow rename <eski> <yeni>                    # referanslar + dosya taşıma
   duflow rm <id> [--force]
   echo '[{"op":"add_child","id":"a","line":"-> \"b\""}]' | duflow apply - [--dry-run]   # toplu (JSON op listesi)
   ```
4. **Bitirince doğrula:** `duflow validate` sıfır hata vermeli. Yeni eklenen node'a giden kenar yoksa `unreachable` çıkar; bağla.
5. Kullanıcıya göstermek için: `duflow ui build -o duflow.html` (tek dosya) ya da `duflow ui serve`.
   PR farkı: `duflow diff main` (metin) / `duflow ui build --diff main`.

## Format (KDL), özet

```kdl
state "deploy.rolling.retry" layer="domain" desc="Deneme sayacı artar" {
  sets "deploy.attempts" "+1"
  -> "deploy.rolling.create_instance" when="deploy.attempts < 3"
  -> "deploy.rolling.rollback"        when="deploy.attempts >= 3"
  on "instance.failed" -> "deploy.rolling.retry"
}
call "api.deploys.create" layer="api" method="POST" path="/api/projects/{id}/deploys" desc="..." {
  check "perm:deploy.trigger" fail=404                      # fail hedefi check tanımından
  check "has_success_build" fail=422 code="no_build" -> "ui.toast.no_build"
  returns 202 -> "deploy.queued"                            # iç çağrıda: returns ok -> "x"
}
action "ui.project.deploy_submit" layer="ui" kind="submit" desc="..." { calls "api.deploys.create" }
event  "instance.healthy" layer="domain" desc="..."         # `on` ile dinlenir
var   "deploy.attempts" type="int"                          # sets ile yazılır
var   "role" type="enum" source="session" values="admin member"   # dış kaynaklı
check "perm:deploy.trigger" desc="..." reads="role" -> "ui.toast.not_found"
root  "ui.login"                                            # giriş noktası; her node bir root'tan erişilebilmeli
view  "deploy_from_ui" from="ui.login" to="deploy.done"     # kayıtlı sorgu, veri değil
```

Kurallar:
- Node türleri: `state` (durum), `action` (click/submit/prompt; `calls`), `call` (endpoint/RPC; `check` + `returns`), `event` (async olay).
- ID `a-z0-9_` + nokta; nokta = hiyerarşi/grup. Dosya: `a.b` → `a.kdl` ya da `a/b.kdl`; `a.b.c…` → `a/b.kdl`. Tanım tek yerde, diğer dosyalar ID ile referans verir.
- Katman `layer="ui|api|domain"` (proje `flow.kdl`'de genişletir).
- Guard `when="..."` string; yorumlanmaz, içindeki isimler tanımlı `var` olmalı.
- Yalnız durum değiştiren şeyler girer; spinner/kozmetik girmez. Kontrol edilmesi gereken her şey `check`.
- `desc` tek satır, projenin dilinde (Duploy: Türkçe). Kod/ID İngilizce.

## Lint hataları (sık)
`dangling_ref` hedef yok · `unreachable` root'tan yol yok · `unknown_var` guard'da tanımsız isim · `var_never_set` okunuyor ama `sets`/`source` yok · `call_no_returns` · `id_path_mismatch` yanlış dosya · `unknown_check`.
