# Ortak — doğal ekip konuşması ve tamamlanmış özel v0

Bu belge, kullanıcının 7 Eylül 2026 tarihli yeni çalışma talebidir. Önceki uzun
oturumun yerine temiz bir görevde **aktif goal olarak başlatılmalı ve uygulanmalıdır**.
Yalnızca plan/rapor üretme. Bir teslim tarihi, çalışma süresi veya toplam token
bütçesi yoktur. Bitiş, aşağıdaki ürün ve teslim koşullarının karşılanmasıdır.

## Goal

Mevcut Ortak çalışmasını kaybetmeden devral; önce değişiklikleri güvenli ve
incelenebilir bir Git teslimine dönüştür. Çalışanların kendi kimliklerini,
ekipteki rollerini ve yetkili oldukları konuşma/iş bağlamını bilerek doğal biçimde
iletişim kurmasını tasarla ve uygula. Kullanıcının bir fikirle başlayıp ekipten
yardım alması, önceki yanıtları başka çalışana yorumlatması, konuşmayı göreve
çevirmesi, gerçek çıktı/artifact alması, insan onayı vermesi ve sonradan kaldığı
yerden devam etmesi aynı kullanılabilir ürün akışında çalışsın. Architecture v0'ın
Office, Employees, Work, Projects, Activity, Memory ve Settings kapsamındaki
kalan boşlukları kapat; mevcut gerçek akışları koru. Özel kurulumun açılması,
kapanması, yeniden başlaması ve kurtarılması tekrarlanabilir olsun. Sonuçları
kısa Türkçe kullanım notları, doğrulanmış kabul sonuçları ve GitHub'daki düzenli
signed commitler/inceleme bağlantısıyla teslim et. Tek başarılı yanıt veya yeşil
unit testi tüm goal'ın tamamlanması sayılmaz.

## Kullanıcının gösterdiği temel eksik

Gerçek kullanıcı konuşması:

1. İnsan: “bir ürün üzerinde çalışacağım, kim yardımcı olur”
2. Ada, ürün yöneticisi/tasarımcı/geliştirici gibi genel roller sayıyor.
3. İnsan: “bunu ingilizceye çevirir misin Bora”
4. Bora, Ada'nın yanıtı yerine bu talep cümlesini İngilizceye çeviriyor.
5. İnsan: “adanın yazdığını ingilizceye çevirmen gerekiyordu”
6. Bora, Ada'nın metnini tekrar göndermesini istiyor.

Kaynak incelemesinde neden görüldü:
- `crates/ortak-runtime/src/postgres/authority.rs` tetikleyen olayın içeriğini okur.
- `crates/ortak-runtime/src/authority.rs::run_spec` bunu `input` yapar;
  conversation/message kimlikleri metin geçmişinin kendisi değildir.
- `runtime/hermes-bridge/ortak_hermes_bridge/hermes_candidate.py` çağrısında
  `conversation_history=[]` ve genel Office sistem mesajı kullanılır.
- Onaylanmış kalıcı bellek eklenebilmesi, yakın konuşma geçmişinin taşındığını
  göstermez. Bu temel kullanıcı beklentisi önceki kabulde kaçırıldı.

Bu kullanıcının hatası veya yalnızca model seçimi sorunu olarak açıklanmamalı.
Önceki “çalışan yerel v0” beyanı çok turlu sohbetin tamamlandığı anlamına gelmez.
Ayrıca görüntülenen `Agent; owner unavailable`/`Offline` etiketlerinin Ortak
Employee kimliği ve gerçek çalışma durumu ile uyumunu incele; sahte owner,
sağlık veya presence verisi üreterek düzeltme.

## Başlangıç ve tek sahiplik

- **Tek yetkili çalışma ağacı:** `/Users/nambse/.codex/worktrees/14b1/ortak.dev`.
  Görevin başlangıç dizini farklı olsa da bütün repository komutlarında bu cwd'yi açıkça
  kullan. Yeni worktree, yeni temiz checkout veya repo kopyası oluşturma; Git'e
  alınmamış büyük çalışma bu dizindedir. Projenin kayıtlı `/Users/nambse/dev/ortak.dev`
  yolu ve eski `a5ed` checkout bu işin yerine geçmez.
- Dal `codex/ortak-v0-delivery`, son doğrulanan HEAD
  `afbb891732a584404e9c21ffc7b5028da2c389d5`. Başlangıçta yeniden doğrula.
  Son status707 değişiklik girdisi,438 untracked
  girdisiydi. Bunlar dosya içeriği/son durum yerine geçen sabit sayılar değildir.
- Origin son gözlemde `git@github.com:nambse/buzz.git`; `buzz-reference` salt
  referans/upstream'dir ve push kapalıdır. Son büyük geliştirme commit/push
  edilmedi. Kullanıcı toparlama, GitHub'a gönderme ve v0 tamamlama hedefini
  onayladı. Uygun signed commit/branch push ve draft PR için tekrar izin isteme;
  force push, otomatik merge, repository silme/yeniden adlandırma yapma.
- Git/hook öncesi `. ./bin/activate-hermit`. Başlangıçta gerçek branch/HEAD/diff,
  aktif derleme ve kaynak sahiplerini doğrula. Eski görevde paralel writer veya
  başka goal başlatma. Bu dosyayı getiren görev devirden sonra ürün kodu yazmayacak.
- Yeni görevde bu Goal bölümünü ve kabul koşullarını kapsayan `create_goal` çağrısı
  yap; `token_budget` verme. Eski görevin goal'ını tamamlanmış sayma. Kullanıcının
  “ultrathink” isteğini dikkatli tasarım ve eleştirel inceleme olarak uygula;
  bunu yeni model adı, sabit bütçe veya çalışma saati olarak yorumlama.
- Eski göreve bağlı heartbeat devir sırasında kaldırılacak. Aynı eski görevi tekrar
  uyandırma veya yinelenen otomasyon oluşturma. Aktif goal üzerinden ilerle.

## Önce okunacaklar ve mevcut doğrulanmış durum

Önce `AGENTS.md`, `ARCHITECTURE_V0.md`, `IMPLEMENTATION_PLAN_V0.md`,
`REMAINING_WORK_V1.md`, `BUZZ_BASELINE.md`, `DEPLOYMENT_STRATEGY_V0.md`,
`UPSTREAM_MAINTENANCE.md`; ardından `CONTINUATION_PROGRESS_2026-09-05.md` dosyasının
**en üst güncel kaydı**, `PRIVATE_V0_RUNBOOK.md` ve bu goal'ın yanındaki
`HANDOFF_CONVERSATION_AND_V0_2026-09-07.md` okunmalı. İlgili VISION/TESTING
kurallarına uy. Eski devam promptunun tarihsel durum, checkout, OAuth ve model
varsayımlarını tekrar başlangıç hedefi yapma; bu belge onları günceller.

Önceden gerçekten çalıştırılanlar: üç taze çalışan ve paylaşılan Hermes OAuth;
merkezi deterministik/anlamsal yönlendirme; gerçek Office yanıtı, iptal ve kalıcı
olay akışı; Work→run→artifact→REVIEW→insan tamamlaması; onaylı proje/konuşma/
çalışan belleği ve Stop/withdraw; seçili Deniz şifreli DM; şema78 temiz kurulum ve
eski44→78 yükseltme; dolu tam yedek, aynı makinede ayrı hedefe fiziksel restore ve
özgün kaynakları tekrar çalıştırma. Bunlar bütün sohbet deneyiminin, bağımsız
makine kurtarmasının veya genel kurulum ürününün tamamlandığını kanıtlamaz.

Son masaüstü paketi eski voice/terminal/mesh yollarını dışlar; özel relay eski
workflow/git/mesh özellikleri olmadan derlenmiştir. Şirket menüsü sabit,
Ortak profil metinleri düzeltilmiş, eski feedback/update menüleri kapatılmıştır.
Ayrıntılı sonuçlar/pinler handoff'tadır. Sonrasında yapılmış kullanıcı mesajlarını
koru; veri/reset/yeniden seed işlemiyle temiz başlangıç görünümü üretme.

## Tasarım ve uygulama dilimleri

### 1. Mevcut emeği toparla ve güvenceye al

Diff'i anlamlı ürün alanlarına ayır, takip edilmeyen dosyaları tek tek sınıflandır:
ürün kodu, regresyon, kalıcı belge, tekrar üretilebilir çıktı ve özel veri.
Körlemesine `git add .` yapma. Secret/OAuth/anahtar/auth.json/env ve özel çalışma
çıktıları Git dışında kalmalı. Geçmiş kaynakları silmeden geliştirmeyi signed
checkpoint commitlerle görünür yap. Uygun branch push'unu erken tamamla;
son kullanıcı kabulünü beklerken tüm emeği yalnızca geçici diskte bırakma.
Başarısız kontrol varsa saklamadan kaydet; kontrol geçmeden final release/PR
hazır beyanı verme. Son teslimde gerçek commit/branch/PR URL'si olmalı.

### 2. Konuşma ve ekip bağlamını ürün olarak tasarla

Önce kısa bir mimari karar belgesi yaz: yakın konuşma bağlamı, uzun konuşma özeti,
aktif iş/proje durumu ve onaylı kalıcı bellek birbirinden nasıl ayrılır ve birlikte
nasıl kullanılır? Sadece son N mesajı her yere eklemekle veya her employee'yi
kanala abone yapmakla yetinme.

Karar ve uygulama şu noktaları çözmeli:
- Çalışanın kalıcı adı/rolü/görevi, kendi gerçek yetenekleri ve izinli ekip
  arkadaşlarının rolleri güncel sunucu verisinden gelsin. Genel “şu mesleklere
  ihtiyacın var” cevabı yerine gerçek ekipten nasıl yardım alınabileceği bilinsin;
  bilinmeyen yetenek/uygunluk veya yapılmamış iş uydurulmasın.
- Doğrudan yanıtlanan mesaj, bağlı thread kökü/ilgili dal ve gerekliyse aynı
  kanaldaki yakın mesajlar seçilsin. “Bunu”, “Ada'nın önerisini”, “ikinci maddeyi”,
  “öncekini kısalt” gibi ifadeler erişilebilir içerikle çözülsün. Gerçek belirsizlikte
  kısa netleştirme sorusu sorulsun; erişilebilir metin yeniden yapıştırılmasın.
- Seçim kanonik mesaj kimliği, yazar Employee/insan kimliği, sıra/zaman, yanıt
  ilişkisi ve ilgili iş/artifact referanslarıyla açıklanabilir olsun. Kesin yanıt
  ilişkisi, zamansal yakınlık ve anlamsal ilgililik arasındaki öncelik tanımlansın.
- Başka çalışanın metni, alıcı çalışanın kendi geçmiş cevabı ya da sistem talimatı
  gibi sunulmasın. İnsan talebi ile alıntılanmış/geçmiş içerik ayrı tutulmalı.
- Thread, kanal, izinli DM, proje ve görev kapsamları karışmasın. Bir konuşmada
  sürmekte olan işi başka odanın yakın zamanlı mesajları yanlış yönlendirmesin.
- Mesaj ve token/byte bütçeleri, kesilme/özetlenme bilgisi ve gerekirse yetkili
  eski mesaj/artifact erişimi belirlenmeli. Sonsuz transcript, sınırsız arama,
  her turda pahalı özet ve görünmez otomatik bellek paylaşımı yok.
- Konuşma/thread kimliği ile bir insan talebinin çalışanları uyandırdığı teslim
  zincirinin ömrünü ayır. Aynı thread'deki yeni insan talebi meşru bir yeni tur
  başlatabilmeli; buna karşılık aynı nedensel zincirde yinelenen çalışan yanıtı
  sayaçları sıfırlayıp sonsuz döngü yaratamamalı. Gerekli mimari netleştirmeyi
  karar belgesine işle; merkezi kalıcı sayaçları atlayarak çözme.
- Çalışan cevap üretirken gelen düzeltme, yeni istek ve başka çalışanın cevabı
  için sıra/iptal/birleştirme davranışını tanımla. Eski yanıtın yeni isteğin
  cevabı gibi gösterilmesini ve eşzamanlı işlerin bağlam karışmasını önle.
- Employee kimliği sohbet oturumuna veya modele bağlanmasın. Model değişimi ve
  worker yeniden başlaması, aynı yetkili konuşmanın anlamlı devamını kaybettirmesin.
  Hermes'in gerçekten desteklediği history/session sınırını kaynakta doğrula.

### 3. Yetki ve kalıcılığı bağlamla birlikte koru

Bağlam Ortak kontrol katmanında sunucu tarafından seçilsin. Alıcı için şirket,
üyelik, DM katılımcılığı, proje/work erişimi ve silinme/revocation kontrolleri
uygulansın. Yeni mesajın snapshot sınırından sonra gelen olaylar rastgele eski
çalışmanın girdisine karışmasın. Tekrar denemede aynı seçilmiş bağlam kimliği ve
sırası korunsun; fakat snapshot yetki yerine geçmesin: geri alınmış izin/gizlenmiş
kaynak için geç başlatma ve teslim davranışı açıkça tanımlanıp doğrulansın.

Kullanıcı metni, geçmiş alıntı, model özeti veya runtime isteği erişim yetkisi,
konfigürasyon değişikliği, tool izni veya yeni wake emri üretmesin. Tarihsel
mention'ları yeniden yönlendirip eski çalışanları uyandırma. Deterministik merkezî
routing, bir mesaj/tek karar, per-root kalıcı sayaçlar ve benzersiz çalışan
rezervasyonları korunmalı. Çalışan kaynaklı konuşma otomatik fan-out başlatmasın.

Şifreli DM geçmişi sıradan RunSpec, genel DB/journal/log/Activity veya başka
çalışanın bellek kapsamına taşınmamalı. Yetkili şifre çözme sınırını ayrı koru.
Bağlam audit'inde içerik kopyalamak yerine uygun kaynak kimlikleri, seçim nedeni,
bütçe ve provenance göster; yalnız yetkili kullanıcı gerekli içeriği görebilsin.

### 4. Gerçek hayata yakın sohbetten işe akış

Konuşma her mesajda sıfırlanan ayrı komutlar dizisi gibi hissettirmesin. Kullanıcı
fikir/amaç anlatsın; ekip gerçek rolleriyle katkı versin; başka çalışana çeviri,
eleştiri veya düzenleme devretsin; talep yetkili ve izlenebilir Work'e dönüşsün.
Gerektiğinde sorumluluk/kabul ölçütü/netleştirme istensin. Basit sohbeti zorla
göreve çevirmek veya yapılmamış işi yapılmış gibi göstermek yok.

Görev ve artifact sürümleri referanslanabilsin; “bunu revize et” doğru sürümü
kullansın. Çalışan çıktısı REVIEW'a gelsin, son insan onayı korunsun. İptal,
kesinti, yeniden bağlanma, bekleyen teslim ve başarısızlığın toparlanması UI'dan
anlaşılır olsun. Çalışanın kimlik/owner/availability etiketlerini Ortak modeliyle
uyumlu hale getir; bağımsız gateway'e bağlı olmaması “Offline” diye sunulmasın.

### 5. v0 ve işletim teslimini tamamla

Architecture v0/REMAINING_WORK kapsamını, güncel gerçek sonuçlara göre yeniden
eşleştir; tamamlananları tekrar başlatma, açık kalanları bitir. Employees
create/configure/activate/disable/re-enable; Work/Projects düzenleme ve inceleme;
Memory izin/provenance/retention; Activity açıklama/iptal/recovery; şirket ve
bağlantı ayarları kullanıcıya anlaşılır olmalı. Gizli değer yerine güvenli seçilmiş
credential reference kullanılmalı; hazır olmayan kontroller başarı taklidi yapmamalı.

Şu anda servislerin Mac yeniden başlatması sonrası otomatik yönetimi yoktur.
Mevcut kurulum için tek, anlaşılır, tekrarlanabilir açma/durum/kapatma/yeniden
başlatma yolu tamamla; her kullanımda yeni image/container/identity üretme.
Kesintide kalıcı kayıtları koru, sahipsiz süreç ve yinelenen worker bırakma.
Mevcut `/private/tmp` bağımlılığını sürdürülebilir kurulum açısından değerlendir;
gerekirse doğrulanmış, geri alınabilir veri taşıma planını uygula. Sadece bu
kuruluma ait veriyi taşı; eski dış stack'i benimseme.

Yeni kaynak/şema/runtime değişikliklerinin etkilediği install, baseline upgrade,
yedek/restore ve rollback sınırlarını doğrula. Önceki aynı-makine G78 sonucunu
yeni değişikliklere veya farklı makineye otomatik atfetme; değişmeyen pahalı
rehearsal'ı gerekçesiz tekrarlama. Eski G78 yedeği yeni OAuth hesabından önce
alındı; eski hesabı geri getirmeden yeniden yetkilendirme/kurtarma yolu gerekli.

### 6. Son kabul ve GitHub teslimi

Önce hedefli, üretim yolunu kullanan/falsifiable regresyonlar; sonra ilgili gerçek
entegrasyon ve kısa native kullanım senaryoları. Değişen relay/db/auth için repo
zorunlu entegrasyonları ve final teslim/PR öncesi `just ci` tamamlanmalı. Yanlış
hedef DB, disk veya ortam problemi test başarısı sayılmaz; çöz veya gerçek
engeli kayıt altına al. Son kod/build/deployed/CI sürümlerinin ilişkisi açık olsun.
İncelemeden ve zorunlu kontroller tamamlanmadan PR'ı merge/release etme.

## Somut kabul senaryoları

1. Yukarıdaki Ada→insan→Bora çeviri örneği **metni yeniden yapıştırmadan** doğru
   çalışır. Bora, son talep cümlesini değil Ada'nın gerçek yanıtını çevirir.
2. İnsan “daha kısa yap / ikinci maddeyi değiştir” dediğinde ilgili önceki çıktı
   korunur. Birkaç araya giren mesaj ve iki ayrı thread doğru kaynağı bozmaz.
3. Ürün fikri sorusunda Ada kendi rolünü ve gerçek ekip katkılarını bilir;
   Bora/Deniz'in sahip olmadığı beceri veya yetkiyi uydurmaz.
4. Açık reply/mention ile yönlendirme doğru çalışır. Gerçek belirsizlikte çalışan
   kısa bir soru sorar; kesin olmayan bağlamı eminmiş gibi uydurmaz.
5. Konuşma→görev→çalışan çıktısı/artifact→revizyon→insan incelemesi/tamamlaması
   UI'dan yürür; “tamamlandı” yalnız model sözüne dayanmaz.
6. Model değişimi, worker restart ve uygulamayı yeniden açma doğru konuşmaya
   devamı korur. Aynı kabul edilmiş istekten çift run veya çift teslim oluşmaz.
7. Kapsam dışı kanal/DM/şirket, geri alınmış üyelik, silinen kaynak ve kötü niyetli
   geçmiş talimat yetkiyi aşamaz. Şifreli içerik sıradan kayıtlara sızmaz.
8. Uzun konuşma bütçeli kalır, özet/kaynak ilişkisi izlenebilir; tarihsel mention
   ve çalışan cevabı sonsuz yönlendirme/yanıt döngüsü başlatmaz.
9. İnsan gerçek kullanımda çalışanın neden cevap verdiğini, neyi referans aldığını,
   neyin beklediğini ve nasıl durduracağını anlayabilir. `owner unavailable`
   gibi miras UI bozuklukları kalmaz; bilinmeyen sağlık dürüstçe gösterilir.
10. Kullanıcı özel kurulumu belgelenen basit yolla açıp yeniden başlatabilir.
    Repo düzenlidir, ilgili testler ve zorunlu CI sonuçları kayıtlıdır; değişiklikler
    GitHub'daki signed commitler/branch/PR üzerinden incelenebilir.

## İş disiplini ve sınırlar

- Zaman sınırı olmaması sonsuz test/araştırma anlamına gelmez. Her dilim için
  değişiklik, kabul koşulu ve bitiş belirt. Geçen kontrolü yalnız yeni değişiklik
  veya somut belirsizlik gerektiriyorsa tekrarla. Gerçek provider denemelerini
  kısa ve amaca yönelik tut; sadece bunu ölçmek için 40 tur konuşma üretme.
- Türkçe, kısa ve dürüst güncellemeler ver. Kaynakta tamam, testte tamam, çalışan
  üründe doğrulandı ve henüz yapılmadı durumlarını ayır. Sonraki iki somut işi
  ve varsa dış engeli kalıcı ilerleme kaydında tut.
- Mevcut kullanıcı yetkisi içindeki rutin kod, test, build, yerel kurulum,
  cleanup, signed commit ve branch push için tekrar onay isteme. Eksik tercih
  varsa bağımsız ilerlerken erken sor; gerekli olmayan credential/model soruları
  ile yeniden başlama. Yeni ChatGPT bağlantısı hazır; model şu an kritik değil.
- Alt ajan gerekiyorsa mevcut platform kurallarına göre somut bağımsız iş ayır;
  ortak şema, Git entegrasyonu, canlı DB/Docker/native ve ağır build tek sahibin
  sorumluluğunda olsun. Yeni kullanıcıya ait task'lar çoğaltma.
- Disk kullanımını takip et. Gerekmeden yeni container/image/checkout üretme.
  Kullanılmayan bu işe ait yardımcıları doğrulayarak kaldır; güncel artifact ve
  gerektiği kadar rollback tut. Global prune, volume silme veya eski test
  ortamlarını temizleme yok. Geçerli restore doğrulama DB'lerini koru.
- Özel DB55433 canlı üründür; yıkıcı fixture yalnız açık disposable55432 içindir.
  Uygulanmış migration'ları değiştirme. PID/port/ad yerine güncel sahiplik ve
  artifact kimliğiyle işlem yap. Kullanıcının son gerçek konuşmalarını koru.
- Cem/Zeynep/Coolify/Hetzner ve diğer projeler korunur. Opaque referans dışındaki
  gizli değerler chat/log/Git/goal'a taşınmaz. Yeni altyapı satın alma, kredi/reset
  kullanma, kontrol edilmemiş public deployment veya dış kişilere mesaj gönderme
  bu yetkiye dahil değildir. Upstream floating auto-merge/deploy yapma.
- Gerçek engel varsa goal tamamlandı deme; platformun goal durum kurallarını
  uygula. Yalnız model limitine çarpmak, acele etmek veya bir alt işin bitmesi
  kullanıcı hedefinin karşılandığı anlamına gelmez.

## Yeni görevde ilk adımlar

1. Bu belge/handoff ve güncel checkout'u oku; `create_goal` ile yukarıdaki hedefi
   zaman/token bütçesi eklemeden başlat. HEAD/diff, çalışanlar ve disk durumunu
   güvenli biçimde doğrula; tamamlanmış OAuth login/test döngüsünü tekrarlama.
2. Mevcut değişiklikleri kaybetmeyecek Git toparlamasına başla ve konuşma bağlamı
   tasarımını üretim yolları üzerinden yaz. Kalan v0 kapsamını kısa bir kontrol
   listesinde güncelle; sadece belge üretip durma.
3. Ada→Bora örneğini bağlam sözleşmesi ve üretim testleriyle düzelt; ardından
   yukarıdaki sohbet/iş, işletim ve teslim kapılarına bağımlılık sırasıyla ilerle.
