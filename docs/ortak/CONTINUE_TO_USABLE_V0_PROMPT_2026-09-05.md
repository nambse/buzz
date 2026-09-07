# Ortak v0 — kullanılabilir ürüne kadar devam görevi

Bu yeni görevde Ortak geliştirmesini devral ve ürünün tanımlı v0 kapsamı gerçekten kullanılabilir olana kadar uygula, entegre et ve doğrula. Kullanıcı önceki uzun görüşme yerine temiz bir görev açılmasını, bütün bağlamın aktarılmasını ve ara teslimlerde durulmamasını istedi. Bu bir plan yazma veya sadece inceleme görevi değildir: kod yaz, çalıştır, hataları gider ve ürünü teslim et.

## Yetki, çalışma biçimi ve devamlılık

- Kullanıcının kodlama, terminal komutları, testler, build, mevcut kapsam içindeki yerel kurulum/güncelleme ve subagent kullanımı için verdiği yetki devam ediyor. Zaten yetkilendirilmiş rutin işler için yeniden onay isteme; subagentlara da bunu açıkça aktar.
- Önceki 2026-09-05 saat12:00 Istanbul durma sınırı bu yeni taleple kaldırıldı. Yeni bir saat veya token bütçesi verilmedi. Tamamlanmış bir alt modül, yeşil unit test veya ilk yanıt tüm görevin tamamlandığı anlamına gelmez.
- Ortamın gerçek izin politikasını kullan. Promptun izinleri veya platform limitlerini değiştirdiğini varsayma; güvenlik mekanizmalarını, kilitleri veya OS izinlerini aşmaya çalışma. Bir komut engellenirse izin gerektirmeyen meşru alternatifle ilerle. Yalnızca zorunlu bir dış girdi eksikse bunu kısa ve açık şekilde belirt; bağımsız işlere devam et.
- Önceki tercih GPT-6 Astra ve ultra düşünme. Desteklenen paralel subagentları somut, bağımsız işlere ayır; entegrasyon ve ortak şema/arayüzlerin tek sahibi sen ol. Model tercihini sessizce değiştirme.
- Türkçe ve kısa ilerleme bilgileri ver. “İstersen devam edebilirim” diye bitirme. Bir kapı geçildiğinde sıradaki bağımlılığa geç. Aynı değişmeyen testleri tekrar tekrar çalıştırarak zamanı doldurma.
- Kalıcı ilerleme kaydı tut: son commit, uygulanmış değişiklik, çalıştırılan test ve sınırları, gerçek çalışan artifact, eksik girdi, sıradaki somut adım. Context compaction veya süreç kesintisinde bu kayıttan devam et; tamamlanmış işleri baştan yapma.
- Başlatan görev, mevcut `ortak-morning-delivery` otomasyonunu bu yeni göreve yönlendirecek. Önce durumunu kontrol et; ikinci bir otomasyon veya eski görevde ikinci bir writer başlatma. Otomasyon henüz yönlendirilmediyse ilk kodlama işine devam et, onu çoğaltma. Bitince otomasyonu duraklat. Platformun sağladığı devam/goal araçlarını yalnızca kendi kullanım şartlarına uygun kullan.
- Gerçek bir dış engel varsa başarı uydurma. Gerekli bilgiyi erken ve bir kez iste, cevap gelene kadar engellenmeyen işleri tamamla. Tüm anlamlı işler gerçekten o girdiye bağlı kaldığında dürüst bir engel kaydı bırak; boş retry döngüsü kurma.

## Başlangıç ve kaynaklar

Yeni checkout, mevcut `codex/ortak-private-mvp` dalından türemelidir. Son ürün/devir tabanı `b9b00a379ee9ae6a967cf40241820048f37adbd9`; bu promptu ekleyen sonraki commit yalnızca devir belgesidir. Başlangıçta HEAD/ancestry/diff kontrolü yap; yanlışlıkla eski varsayılan dala veya Buzz upstream'e dönme.

Önce kendi checkout'unda şu belgeleri oku:
1. `AGENTS.md`, `docs/ortak/ARCHITECTURE_V0.md`, `docs/ortak/IMPLEMENTATION_PLAN_V0.md`.
2. `docs/ortak/CLI_HANDOFF_2026-09-05.md`, `docs/ortak/REMAINING_WORK_V1.md`, `docs/ortak/ACTIVATION_COMPOSITION_GAPS.md`.
3. `docs/ortak/DEPLOYMENT_STRATEGY_V0.md`, `docs/ortak/BUZZ_BASELINE.md`, `docs/ortak/UPSTREAM_MAINTENANCE.md`.
4. Etkilenen yüzeyler için VISION/TESTING belgeleri; kanıt gerektiğinde `docs/ortak/OVERNIGHT_DELIVERY_PLAN_2026-09-05.md` ve ilgili runtime runbook'ları.

Bu yeni devam talimatı yalnızca eski zaman sınırını ve görev sahipliğini değiştirir; ürün mimarisi ve doğrulama şartlarını kaldırmaz. Eski ledger içindeki “henüz yapılmadı” tarihsel notlarını güncel handoff ve kaynakla karşılaştır.

Eski görev `01a06f05-497a-7380-a611-75b7d9432d60`, eski worktree `/Users/nambse/.codex/worktrees/a5ed/ortak.dev`; kayıtlı proje `/Users/nambse/dev/ortak.dev`. Eski görevi resume/fork ederek tüm geçmişi tekrar taşıma ve eski checkout'a eşzamanlı kaynak yazma. Yeni checkout'taki helper/bundle/config yollarını doğrula; eski mutlak build yollarının yeni checkout'a ait olduğunu varsayma.

## Doğrulanmış mevcut durum

- Yerel kaynak commitleri: `2eac15f` temel entegrasyon, `f23c9fd` manuel Work/kurtarma, `d07f55c` çalışan iş kuyruğu/topluluk ayrılması, `5c285d2` taze aktivasyon kabulü ve sentetik Hermes HTTP, `1dea0d0` ortam tabanlı credential-reference resolver, `b9b00a3` öğlen devir kaydı.
- Öğlen gözleminde özel backend `5c285d2` kaynak build'iydi; native paket `d07f55c` kuyruk build'iydi. `1dea0d0` resolver kaynakta test edildi, çalışan production saga'ya bağlanmadı. Bunlar güncel süreç gözlemi değildir; şimdi tekrar kimliklerini doğrula.
- Özel veritabanı migrasyon56'ya yükseltildi. Ada (`ada-private`) draft, aktif revizyon yok, merkezi routing kapalı. Gerçek model/provider seçilmedi ve gerçek çalışan→Office yanıtı oluşmadı. Mevcut fake/sentetik başarıyı aktivasyon için kullanma.
- Manuel Work API akışı doğrulandı: bir proje, bir tamamlanmış iş,8 işlem receipt'i,7 history satırı; orijinal akış version1→7, tekrar7→7; assignment/run/outbox/routing decision sayıları sıfırdı.
- Work19 PostgreSQL, imzalı API12 PostgreSQL, headless UI4 test geçti; screenshotlar incelendi. Aktivasyon25 saga/25 control unit ve14 farklı PostgreSQL vaka; yeni resolver6 odaklı test ve scoped Clippy geçti. Bu grupları çakışmaları yokmuş gibi tek toplam yapma.
- Gerçek Hermes controller+journal+Docker executor+pinned AIAgent/SDK, yerel sentetik HTTP ile sınandı:3 Responses isteği+2 catalog404, sıfır gerçek provider isteği, fixture cleanup doğrulandı. Testin endpoint/OS-header seam'leri açıkça belgeli; ek test audit'inin uname reddi production SDK arızası kanıtı değildir.
- Native uygulama build edildi ve açıldı; kısa süreli TCP gözlendi, sonra bağlantı görülmedi. Native görsel kullanım, authenticated WebSocket ve otomatik reconnect henüz doğrulanmadı. CLI ile özel owner'ın kanalı okuması başarılıydı. UI'nın sorunsuz çalıştığını process/PID üzerinden iddia etme.
- Full repository `just ci`, PR ve push bu checkpoint için yapılmadı. Yeni değişikliğe uygun testleri çalıştır; release/PR öncesi gerekli tam kalite kapısını tamamla.

## Özel kurulum, pinler ve korunacak veriler

- Korunan özel state: `/private/tmp/ortak-private-20260905`;0700 dizin ve0600 gizli dosyalar Git dışındadır. Marker, sahiplik, config ve çalışan süreç kimliklerini mevcut helper'larla doğrula. Eski PID'lere güvenip process öldürme; ikinci API/relay/worker başlatma.
- Özel portlar: relay3038, health8089, metrics9198, API8787, PostgreSQL55433, Redis56382, MinIO9008, Honcho8009. Docker socket `unix:///Users/nambse/.docker/run/docker.sock`.
- Destructive fixture testleri yalnızca açıkça seçilmiş disposable PostgreSQL55432'yi kullanır. Özel55433 veya eski dış servisler test reset hedefi değildir. Daha önce uygulanmış SQL migration dosyalarını/checksumlarını değiştirme; yeni migration ekle.
- Hermes source `29112bef099274229cadff79cdff7bf7b99c4b77`; worker `sha256:623fae9e3b38c75bc3cb94f73bc3d1c303bc3ed6a77765eb51fc17b54cc90b18`; controller `sha256:ef9a9d2a7446d9e13cdbf94cf1a2152011b5a72050e450d500356f059852d7b1`. Guard/source değişirse yeni artifact üret ve ilgili gerçek constructor/loop/containment kapılarını yeniden doğrula; eski kanıtı yeni image'a atfetme.
- Honcho3.1.1, SQLite3.53.4 ve diğer pin/receipt ayrıntıları runtime belgelerinde. Upstream gözlenen/reviewed/deployed revizyonları ayır; floating auto-merge/auto-deploy yapma.
- En son başarılı database-only backup: `/private/tmp/ortak-private-20260905/backups/20260905T083500Z_952d0c34d48f462ba1d3268d872a5438/manifest.json`.537977 bytes; SHA256 `e737171d4fa1177edba41c26d03b98a0dc48ec0a23952550e1ca2948ee6b9154`; restore DB `ortak_verify_7a359a24f12a4a8795768df594c74f84`;103 table count/migration/schema eşleşti. Bu tüm sistem kurtarma kanıtı değildir. Önceki başarılı/başarısız receipt ve restore DB'leri koru.
- Cem/Zeynep, eski Coolify/Hetzner kaynakları, profil/kimlik/memory/volume/anahtarlar korunur. Onları örtülü adopt etme, etkinleştirme veya silme. Taze izole kaynaklarla ilerle.
- Git/hook öncesi Hermit'i aktive et; commitleri `git commit -s` ile oluştur. Pinned Rust1.95.0. Önceki registry cache `/Users/nambse/dev/ortak.dev-worktrees/buzz-import-2026-09-05/.hermit/rust`; build target `/private/tmp/ortak-root-build-target`. Yeni checkout için build sahipliğini/yollarını açıkça seç; tercihen ayrı target, cache devralınacaksa yalnızca tek writer ve korunacak çalışan artifactleri doğrulayarak. Ağır Cargo/Docker/native build'leri seri çalıştır. Son gözlemde disk yaklaşık1.5GiB boştu; kapasiteyi şimdi ölç. Sadece bu işe ait yeniden üretilebilir eski build çıktılarını gerekçeli temizle; geniş prune yapma.
- Gizli değerleri chat, prompt, log, Git, screenshot veya manifest'e yazma. Manifestlerde sadece opaque credential reference bulunur. API anahtarını sohbetten isteme; kullanıcıdan sağlayıcı/model ve güvenli biçimde yapılandırılmış reference/path bilgisini al.
- Yeni ödeme/altyapı satın alma, usage-reset kredisi kullanma veya gözden geçirilmemiş public deployment yok. Hedef önce kullanılabilir özel kurulum. Ürünün açık uçlu dış iletişim özellikleri mesaj göndermeyi kullanıcı adına kendiliğinden yetkilendirmez.

## İlk somut işler ve tam kapsam

Öncelik gerçek dikey akıştır. Aşağıdaki sırayı bağımlılığa göre uygula; bağımsız işleri paralelleştirebilirsin.

**A — Production aktivasyonunu birleştir.**
Gerçek, company/community/employee/channel sınırına bağlı OfficeIdentityAdapter geliştir; signer public-key kanıtı, güncel membership ve idempotent profil yayını sağla. Mevcut EnvCredentialResolver'ı doğru caller-authorized örnek ve owning adapter referanslarıyla bağla. Ortam değişkeninin varlığı provider sağlığı değildir.

Hermes yalnızca hazırlanmış profil Adopt ediyor; Honcho yürütülebilir memory I/O için orijinal extension-created receipt ve açık roundtrip witness istiyor. Saga tek acquisition mode kullanıyor. Gap belgesindeki taze hazırlanmış kaynakların adopt edilmesi yaklaşımını tutarlı biçimde tamamla veya daha iyi bir tasarımı bütün request/receipt/persistence/compensation sınırlarında uygula. Honcho'nun asıl sahipliğini/native ID'lerini koru; Adopt compensation hiçbir zaman kaynağı silmesin. Taze DB-issued ActivationTarget, generation/baseline/attempt kontrolleri ve deferred commit-time expiry korunsun. Elle active revision/başarılı health receipt yazma.

Gerçek PostgreSQL ve gerçek adapterlarla çalışan default-off, durable/retryable saga runner ekle. Eksik signer/membership/provider/memory witness aktivasyonu kısmi state bırakmadan reddetsin.

**B — Gerçek Office döngüsünü çalıştır.**
Server-owned kanal/çalışan seçimini ingress ve routing'e bağla; eksik inbox kayıtları için bounded/idempotent stored-event reconciliation ekle. Gerçek provider/model/credential seçimi için gerekli bilgiyi erken iste; bu yanıtı beklerken A ve bağımsız işleri bitir. Eski profillerden anahtar keşfedip kullanma.

Aktivasyon ve yetki kapıları geçince yalnızca seçili izole çalışanı/cohort'u aç. Bir insan mesajı→tek routing decision/run→gerçek Hermes sonucu→ordered Activity→tek imzalı Office yanıtını kanıtla. Direct-name dispatch, kapsam dışı girdiler, untargeted disabled-semantic silence, employee-origin döngü önleme ve unsupported DM davranışı korunmalı. Gerçek cancellation, kayıp acknowledgement, worker/bridge restart, cursor replay, aynı imzalı byte'larla delivery retry ve scoped memory idempotency doğrulansın.

**C — Gerçek kullanılabilirliği doğrula.**
Office/Employees/Activity ve native masaüstünü gerçek backend ile kullan: onboarding, bağlantı kopması/yeniden bağlanma, reload/cursor, hatalar ve cancellation görülebilir olsun. Kalıcı cursor recovery ile realtime push'u tamamla; mevcut Activity polling bu kapıyı kanıtlamaz. Başka company/audience/role istekleri reddedilsin. Approval/resume yalnızca runtime gerçekten destekliyorsa sunulsun. Headless fixture veya çalışan process gerçek native kullanımın yerine geçmez; gerekli OS erişimi yoksa onu aşma, bağımsız işlere devam ederek eksik görsel kapıyı açık kaydet.

**D–G — İlk yanıtı bitiş sayma; kalan v0'ı tamamla.**
REMAINING_WORK_V1 ve Architecture kapsamını daraltmadan devam et:
- Sınırlı ve anlamlı semantic routing; izinli conversation/project/employee memory scope, provenance/redaction/retention ve inspect/forget; gerçek capability ve cross-scope isolation. Güvenilir sunucu taraflı DM katılımcı çözümleme/decryption'ı gerçek yetki ve capability kapılarıyla tamamla; B'deki unsupported-DM davranışı geçici güvenli sınırdır, tamamlanmış v0 kanıtı değildir.
- Conversation→atanan Work→gerçek run→artifact→REVIEW→yetkili insan completion; kriterler, düzenleme, reassignment/release, dependency ve decomposition akışları. Runtime sonucu insan kriterlerini otomatik karşılamasın.
- Gerçek runner üzerinde create/adopt/update, progress/retry/compensation, activate/disable/re-enable çalışan yönetimi.
- Tekrarlanabilir private install/launch/upgrade, `b1f6b7ef` baseline'dan upgrade, servis lifecycle, full-stack backup/restore (Postgres+object storage+Honcho+journals/profiles+secret-reference recovery).
- Merkezi routing soak sonrası eski bağımsız wake yollarını ve kapsam dışı Buzz yüzeylerini kontrollü temizle; Ortak branding/onboarding/settings ve upstream pin bakımını tamamla.
- İlgili production-seam regresyonları, gerçek entegrasyon ve kullanıcı akışları; teslim/PR öncesi full `just ci` ve değişen relay/db/auth için gerekli gerçek integration testleri.

## Bitiş tanımı ve son teslim

“Tamam” ancak tanımlı v0 acceptance kriterleri uygulanmış, gerçek bağımlılıklarla çalıştırılmış ve kullanıcının özel Ortak kurulumunu açıp çalışanla konuşması, iş vermesi, sonucu/artifact'i incelemesi ve servisleri yeniden başlatması doğrulanmışsa denebilir. Kod mevcut olması, mock UI, sahte provider, yalnızca DB backup veya ilk başarılı yanıt yeterli değildir.

Son teslimde kısa Türkçe açıklama ve güncel kalıcı runbook ver: nasıl açılır, gerçekten çalışan akışlar, source/build/deployed sürümleri, testlerin sınırları, backup/restore ve rollback, kalan gerçek engel varsa tam olarak ne olduğu. Hazır olmayan maddeyi tamamlanmış işaretleme. Ürün hazır olduğunda devam otomasyonunu duraklat ve çalışma ağacını/signed commitleri doğrula.

Şimdi mevcut durumu hızlıca doğrula, A'yı somut bağımsız alt işlere böl ve uygulamaya başla.

---

Prompt düzeni için kaynak: [OpenAI — hedef, başarı ölçütleri ve devamlılık yönergeleri](https://developers.openai.com/api/docs/guides/latest-model). Ürün gerçekleri yukarıdaki repository handoff ve doğrulama kayıtlarından alınmıştır.
