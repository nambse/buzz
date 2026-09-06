# Yeni göreve teknik devir — 7 Eylül 2026

Bu kayıt **gözlem**, process öldürme veya eski receipt'i yeniden yürütme yetkisi
değildir. Kaynakları ve çalışan sahiplerini işlem öncesi doğrula. Asıl kullanıcı
hedefi `GOAL_CONVERSATION_AND_V0_2026-09-07.md` dosyasındadır.

## Çalışma alanı ve kaynak durum

Aynı mevcut dirty checkout kullanılacak:
`/Users/nambse/.codex/worktrees/14b1/ortak.dev`, dal `codex/ortak-v0-delivery`,
HEAD `afbb891732a584404e9c21ffc7b5028da2c389d5`.
Son büyük çalışmada707 status girdisi/438 untracked vardı; bu iki yeni devir
belgesi de henüz commit edilmedi. Origin `git@github.com:nambse/buzz.git`.
Hiçbir son geliştirme commit/push/PR tamamlandı diye varsayılmamalı.
Eski `CONTINUE_TO_USABLE_V0_PROMPT_2026-09-05.md` tarihsel; eski checkout,
şema56/draft/provider-bekleniyor bilgilerini yeniden uygulama.

İlgili üretim yolları:
- `crates/ortak-runtime/src/postgres/authority.rs`: kanonik tetikleyici içerik.
- `crates/ortak-runtime/src/authority.rs::run_spec`: yalnız tetikleyici body,
  conversation_ref/reply_to_message_id ve başlangıçta boş memory_context.
- `crates/ortak-runtime/src/memory_context*`: freeze/retry/onaylı recall;
  sıradan konuşma geçmişiyle aynı şey değil.
- `crates/ortak-control/src/runtime.rs`: kapalı RunContext/RunSpec sözleşmesi.
- `runtime/hermes-bridge/ortak_hermes_bridge/service.py`: context alanı allowlist.
- `runtime/hermes-bridge/ortak_hermes_bridge/hermes_candidate.py`: genel Office
  promptu ve `conversation_history=[]`. Sadece Python'a history eklemek yetmez;
  yetkili kaynak seçimi, wire, kalıcılık, replay, executor ve installed-image
  sınırlarının tamamını ele al.
- `desktop/src/features/ortak`, profile/Office/thread gösterimi ve employee
  eşlemeleri: owner unavailable/yanlış offline etiketi için inceleme noktaları.

## Özel durum ve çalışan kurulum

State: `/private/tmp/ortak-private-20260905`.
Runtime: `/private/tmp/ortak-hermes-v0-private-20260905`.
Şirket: `a4013353-a84d-49a1-8d2b-10a1caf896fe`.
Kanal: `f6bcbca6-9974-4792-8f2c-e19718f6bc11` (`ortak-private`).
Community: `55bebe0f-90f0-44a2-a021-3b69fbb520a6`.
Üç employee: `ada-private`, `bora-private`, `deniz-private`.
Native app data:
`/Users/nambse/Library/Application Support/dev.ortak.private20260905`.
Şifreli depo `ortak-encrypted-dm-v1/ciphertext.sqlite`, user_version2.
Şifreli Deniz kanalı `be203245-5ca3-4a47-9d88-2c20fc65622a`.

Portlar: Office3038, health8089, API8787, canlı PG55433, disposable test PG55432,
Redis56382, MinIO9008, Honcho8009, controller8650. Docker socket
`unix:///Users/nambse/.docker/run/docker.sock`. Kullanıcı son gerçek sohbetlerini
bu kurulumda yaptı; onları silme/reset etme. Destructive test canlı55433'te olmaz.

Son rollout kökü:
`/private/tmp/ortak-private-20260905/rollouts/schema77-121365f433e34b52ac9cb77558f6e694`.
Adı schema77 olsa da çalışan şema78'dir.
`current-owners78-final.json` SHA
`d76cc84ab431d49a033602fb847c7e2af2534d691cd71a80ac6d313941e692d5`.
Son gözlenenler: relay70748, API42972, management43097, worker71256, native72325.
Önceki görevin terminal session numaralarını yeni görevde kullanılabilir sayma.
Sinyalden önce UID/start/cwd/yüklenmiş executable inode/hash doğrulanmalı.
Önceki worker43191 kaybolmuştu; eski terminal sonucu alınamadığından sebebi
bilinmiyor. Aynı artifact/launcher ile yeniden çalıştırıldı; hata nedenini uydurma.

Relay build:
`/private/tmp/ortak-private-20260905/artifacts/backend78-pruned-bfdf0d3a61a94a02a79de44a4b13e43a`.
Receipt SHA `6fbf50d15d5ac1d9971f704f02e5d78cde268d5c066aa5c6f47cb3a90d2a3ac3`.
Sadece relay/admin yeniden derlenmiş, diğer binary'ler korunmuş kopyadır.
Çalışan API/worker/management hâlâ `backend78-timestamp-d7310263619b4de389883c4b3a7fb6f5`
altındaki aynı artifact'lerdir. Güncel memory wire dosyalarındaki iki eşdeğer
`is_multiple_of` lint düzeltmesi son backend build'den sonra yapıldı; source ve
deployed provenance ayrımını koru.

Native build/retained bundle/launcher:
`/private/tmp/ortak-v0-evidence/native78-final-97a97244333048f4b586e9270878c62e`.
Binary SHA `ca6ae5d8c723fb6a45199f514b98d7ad931d1865789349ff1a22caf1025c26d9`.
Launcher `launch-native78.py`, SHA
`e797840ae3b9b01e57aeab4ad14f9be8bf8cad7a0404296ff3609396597b0913`.
Gerçek binary checkout'un
`desktop/src-tauri/target/ortak-private-native/debug/bundle/macos/Ortak Private.app/Contents/MacOS/buzz-desktop`
yolundadır. Repo helper `scripts/ortak/private_native_services.py` hash'i launcher'da
pinlidir; değiştirdiğinde eski launcher çalışıyor varsayma. Native build için
`node desktop/scripts/ortak-private-native.mjs build`; private relay için
`--no-default-features`. Standart Tauri build eski özellikleri tekrar içerebilir.

## Hermes ve hesap

Son giriş tamamlandı; **yeniden login veya API key isteme**.
Paylaşılan OAuth owner Ada'dır, diğer iki employee ve scorer açıkça bu store'u
kullanır: `/private/tmp/ortak-hermes-v0-private-20260905/oauth/ada-private`.
Opaque ref `secret://ortak-private-20260905/ada-codex-oauth-v0`.
6 Eylül22:15 UTC Sol/high gerçek probe
`18cb73b9-748a-4418-a262-5bd547c97927` completed oldu.
`oauth-account-switch-9a86cb2269e64baabaf39088ce005bdd/completion.json` ve
`probe-completed.json` private state altındadır. Account/access hash'lerini veya
token değerlerini kullanıcıya/Git'e taşıma. Giriş container'ı `--rm` ile kaldırıldı.

Deployed Hermes source `29112bef099274229cadff79cdff7bf7b99c4b77`.
Controller name `ortak-hermes-shared77-121365f433e3`, image
`sha256:4cea528012f51086598e7898d3d3e9264c0fe710aba6f142cad1284a410f9361`.
Worker image
`sha256:80aaa3d95b6abb4105f849e33bf4650653718be14fd274e211ee45bd26d75cee`.
Honcho name `ortak-honcho77-121365f433e3`, image
`sha256:fb13e7f8fa0ae66e02b1097d89acfee23ea4c169610fe1494a949fde86db1dc3`.
Scorer name `ortak-semantic-d3-luna76-c16d148e2229`, image
`sha256:d36af74d5518c5f47f1cdd1096e9479e2aed5227f062febff5561e6b997a5d2e`.
Controller config `<rollout>/controller/config.json`, SHA
`ba6df6fa1525d43e714ba0f768152afb5144fe8370ce8f41be665c82872d65da`,0444 immutable
public dosya. Standart profile_probe CLI0600 istediği için bu dosyada doğrudan
çalışmaz; sırf CLI'ye uysun diye chmod yapma. Önceki tek probe hash doğrulanmış
public config okuması ve normal private service-token loader ile kabul edildi.
Runtime journal `/private/tmp/ortak-hermes-v0-private-20260905/state/journal.sqlite`.
Tam Docker Env veya secret dosyası içeriği çıktılamadan seçilmiş metadata'yı oku.

Son upstream gözlem/review kayıtları continuation/upstream belgelerinde.
Eski/korunmuş image yalnız konteyner listesinde görünmüyor diye gereksiz sayılmaz;
rollback veya sabit reader referansını kontrol et. Floating upstream kurma.

## Yedekleme ve kalıcılık

G78 frozen registry `2805d70cb1674acd8719b119fd886a0d` ile tam capture:
`/private/tmp/ortak-private-20260905/recovery-bundles/c9705a580a8149668143f31079847123/manifest.json`,
SHA `891cb3bf94844ee97e633d251906e966464e70cf3990f0460689810f2f984115`.
Offline restore `bbd0fc8063d34abe81aa06730d5c6600`, manifest SHA
`69888b1f7b99c18235e9f2ceff9176674cfc9e6ded7c25b123ef268671525083`.
İki DB, volumes, protected journal, native ciphertext,16 workspace entry
fiziksel olarak doğrulandı. Özgün servisler tekrar açıldı; restore hedefleri aktive
edilmedi. **Bu yedek yeni ChatGPT girişinden önceydi.** Eski OAuth snapshot'ını
sessizce yeni hesabın üstüne koyma; farklı makine DR kanıtı gibi sunma.

Son güncel selection `<rollout>/recovery-selection78-final`, deployment SHA
`5e31d4da5cb90971b000ebd46d142c144666bd3fbcde8865366e2d9f2afa8013`.
Read-only preparation
`/private/tmp/ortak-private-20260905/recovery-preparations/86866d8a6bc34104b8c3b2ac27fbbb2f/preparation.json`
başarılıdır; yeni capture/restore yapılmadı. Recovery source pointer'ları
`scripts/ortak/private_recovery_inventory.py` ve `recovery_native_ingress.py`
güncellendi. Her yeni generation için yeni gözlem/selection gerekir; eski
tek-kullanımlık root launch intent veya pause helper'ını körlemesine çalıştırma.

Seçilmiş workspace grant'in son gözlenen son kullanım zamanı
2026-09-07T01:37:11Z; geçmiş kabul yetkisini sonsuz dosya erişimine dönüştürme.
Yeni kullanım süresi gerekiyorsa açık ve somut yetki sınırını kullanıcıyla çöz.
Süresi dolmuş employee-memory olgusu/Stop edilen yayın otomatik yenilenmez.

## Kontroller, eksikler ve kaynak kullanımı

Son scoped kontroller: private relay4 üretim testi, seçilmiş dependency graph'lar,
scoped Clippy, native identity14+1 eski ignored, company React3, private policy2,
TypeScript, native gerçek policy probe ve gerçek UI kontrolü. Full `just ci`
yapılmadı. Önceden doğrulanmış senaryoları yeni bağlam tasarımının etkilediği
ölçüde yeniden çalıştır; önceki fixtureları canlı başarı diye kullanma.

Son artifact küçülmesi: relay127,475,984→87,638,000 byte;
native165,181,616→110,505,136 byte. Son geçişte yeni image oluşturulmadı;
login ve iki offline reader container'ı kaldırıldı. Hiç deploy edilmemiş ara
staging'den263,789,760 byte binary silindi; receipt korundu. Restore DB/volume'lar
ve ilgisiz projelerin kaynakları korunuyor. Disk en son yaklaşık23GiB boştu,
başlangıçta yeniden ölç.

Python: `/Users/nambse/.pyenv/versions/3.12.8/bin/python3`.
CARGO_HOME: `/Users/nambse/dev/ortak.dev-worktrees/buzz-import-2026-09-05/.hermit/rust`.
Son paylaşılan target: `<checkout>/desktop/src-tauri/target/ortak-private-native`.
Cargo işleri seri,2 job, incremental kapalı/debug0 kullanıldı. Eski global
`/private/tmp/ortak-v0-build-target` silindi; var sanma. Ağır build başlamadan
çalışan native executable'ın üzerine yazılmasını ve disk taban sınırını önle.
Yeni bakım scriptlerini sonsuz log/retry veya process tree bırakarak çalıştırma.

En büyük açıklar: konuşma/ekip bağlamı (kullanıcı örneğiyle doğrulandı), günlük
sohbetten işe deneyimi, Mac sonrası tekrarlanabilir servis yönetimi/kalıcı kurulum,
Git toparlama/push/PR ve zorunlu final CI. Bu devirle hiçbirini yapılmış sayma.
