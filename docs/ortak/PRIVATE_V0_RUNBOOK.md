# Yerel Ortak kullanım notları

Bu Mac’teki güncel uygulama **Ortak Private**, kalıcı kurulumu
`~/.local/share/ortak/private-v0` altındadır. Önce Docker Desktop’ı açın:

```sh
~/.local/share/ortak/private-v0/bin/ortak open
```

Bu komut kayıtlı servisleri ve aynı operatör kimliğiyle uygulamayı açar. Tekrar
çalıştırmak yeni çalışan, OAuth bağlantısı veya veri deposu oluşturmaz. Eski
`/private/tmp` başlatma tarifleri kullanılmaz. Kaynak kodu ve signed commitler
korunmuştur; geçici dizin kaybı yeni kalıcı kurulumla giderildi.

## Günlük akış

- **Office:** `ortak-private` kanalında Ada’ya ürün planlama, Bora’ya İngilizce
  çeviri, Deniz’e Türkçe metin işleri verebilirsiniz. “Ada, …” gibi açık hitap
  veya mention kullanın. İsimsiz mesajlarda merkezi yönlendirici sıfır, bir ya da
  iki çalışan seçebilir. Mesajın **More actions → View routing decision** menüsü
  seçimi ve gerekçesini gösterir.
- **Thread:** önceki yanıtın altındaki **Reply** ile sürdürün. Ada’nın metnini
  tekrar yapıştırmadan Bora’dan çevirmesini veya kısaltmasını isteyebilirsiniz.
  Farklı konuları ayrı thread’lerde tutun. Çalışan yanıtları kendiliğinden yeni
  çalışanları uyandırmaz.
- **Work:** mesajın **More actions → Promote to Work** menüsünü ya da
  **Projects & Work → New work item** formunu kullanın. Proje, açıklama ve kabul
  ölçütlerini kaydedin; çalışan atayın, **Ready** durumuna alın ve **Start
  execution** seçin. Çıktı **Review** durumuna gelir. **Open text deliverable**
  ile inceleyip ölçütleri ve gerekli incelemeyi onayladıktan sonra **Completed**
  kaydedin. Revizyonda önceki artifact yetkili bağlam olarak seçilir.
- **Employees:** ad, rol ve kayıtlı durum çalışan kimliğine aittir. **Manage
  prepared employees** içindeki hazır seçenekler model/akıl yürütme ayarlarını
  gerçek bağlantı, bellek ve imzalayıcı kontrollerinden geçirerek değiştirir.
  Şu an Ada ve Deniz Sol/high, Bora Luna/high kullanır. **Disable** yeni işi
  engeller; eski bekleyen işleri yeniden etkinleştirmek canlandırmaz.
- **Activity:** ilgili run’ı seçerek kaynakları, kullanılan bellekleri, olayları
  ve teslim durumunu görün. **Cancel run** önce iptal isteğini, sonra worker’ın
  durdurma onayını gösterir. Yanıtın üretilmesi ve Office’e teslimi ayrı kayıtlardır.
- **Memory:** kendi Office mesajınızın menüsünden konuşma veya çalışan belleğini
  inceleyin. Düzenlenmiş metni, çalışanı, kapsamı ve bitiş zamanını açıkça seçin;
  önce onaylayın, ardından ayrıca yayımlayın. Ada için bu Office/proje kapsamları
  seçilidir. **Stop using** yeni kullanımı durdurur ve uzak depodan kaldırılmasını
  izler; onay/kullanım geçmişi kalır. Önceden gönderilmiş yanıtlar geri alınmaz.
  Konuşma notları proje içindeki **Inspect conversation memory**, çalışan notları
  **Review saved memory** üzerinden de durdurulabilir.

Deniz’in doğrudan görüşmesi şifrelidir; ayrı **Encrypted message** alanını
kullanır. Pencereden ayrılınca açık metin görünümü kilitlenir. Şifreli konuşma
normal Office/Activity kayıtlarına taşınmaz. Teslimi belirsiz kalan bir gönderim
varsa arayüz saklanan gönderim için tekrar deneme/kurtarma kontrolleri sunar.

Files kabulü seçilmiş salt okunur dosyadan gerçek artifact üreterek doğrulandı.
Günlük kullanımda çalışanların dosya/tool izinleri boştur. Yeni dosya erişimi
operatörün açık kaynak seçimini gerektirir; rastgele yerel dosya erişimi yoktur.

## Bağlantı, kalıcılık ve kurtarma

```sh
~/.local/share/ortak/private-v0/bin/ortak status
~/.local/share/ortak/private-v0/bin/ortak restart
~/.local/share/ortak/private-v0/bin/ortak stop
```

Kısa kesintiden sonra **Employees → Refresh**, **Refresh work** veya
**Reload timeline** kullanın. `status` kaynak kimliklerini ve bekleyen işleri
kontrol eder; OAuth yüklemez veya model çağrısı yapmaz. Yedi container ve dört
kullanıcı servisi aynı kaynaklarla yeniden başlatılmıştır.

Ada, Bora, Deniz ve merkezi anlamsal yönlendirici bu kurulum için açıkça
paylaşılmış tek Hermes OAuth bağlantısını kullanır. Yeni giriş gerekirse
Hermes’in tarayıcı yetkilendirmesi yapılır; token/anahtar sohbete yazılmaz.

Özel yapılandırma, OAuth, dosya kaynakları, servis kayıtları ve yedekler kalıcı
kurulum dizinindedir. Native uygulama verisi
`~/Library/Application Support/dev.ortak.private20260905` altındadır. Bu
konumlar derleme önbelleği değildir. Genel Docker prune veya eski dış
Cem/Zeynep kaynaklarına yönelik temizlik yapılmaz.

Yedek alma, gerçek çevrimdışı geri yükleme doğrulaması ve sınırlamalar
[işletim rehberinde](PERSISTENT_PRIVATE_OPERATIONS.md). Tam Mac yeniden başlatma
kabulü ve klonlanmış runtime’ı etkinleştirme henüz doğrulanmamıştır; servis
restart’ı ve çevrimdışı veri restore’u bu sonuçların yerine geçmez.
Güncel kod/build/test/dağıtım kayıtları
[devam günlüğündedir](CONTINUATION_PROGRESS_2026-09-05.md).
