# Yerel Ortak kullanım notları

> **7 Eylül yeniden başlatma sonrası:** Aşağıdaki `/private/tmp` tarifleri artık
> çalıştırılmamalı; bu geçici dosyalar temizlendi. Kod ve Docker volume'ları duruyor.
> Kullanıcı eski test kimliklerini/OAuth/verilerini yeniden oluşturmayı onayladı.
> Kalıcı kullanıcı dizininde temiz kurulum ve yeniden başlatma kabulü hazırlanıyor.
> Aşağıdaki bölümler önceki kurulumun tarihsel kaydıdır.

Bu kurulum bu Mac'teki **Ortak Private** uygulamasıdır. Office adresi
`ws://localhost:3038`, ürün API'si `http://127.0.0.1:8787` ve şirket
`a4013353-a84d-49a1-8d2b-10a1caf896fe` olarak sabittir. Ada, Bora ve Deniz bu
kurulumun ayrı çalışanlarıdır. Eski Cem/Zeynep kaynakları kullanılmaz.

## Uygulamayı yeniden açma

Arka plan hizmetleri çalışırken aşağıdaki komut aynı kurulumu ve yetkili yerel
kimliği kullanarak pencereyi açar. Uygulama zaten açıksa tek pencere mekanizması
devreye girer. Bu komut image oluşturmaz veya verileri sıfırlamaz.

```sh
/Users/nambse/.pyenv/versions/3.12.8/bin/python3 \
  /private/tmp/ortak-v0-evidence/employee-native79-c8e5910cb3b24fe2965270ea9362cccc/launch-native79.py
```

Bu, masaüstünü açan komuttur; Docker Desktop ve kapatılmış arka plan hizmetlerini
kurmaz. Tam hizmet yeniden başlatması, güncel sahiplik kaydı ve dondurulmuş
operatör tarifleriyle yapılır. Eski kayıtlardaki PID'lerle işlem sonlandırmayın.
Mac yeniden başladıktan sonra otomatik açılan bir servis yöneticisi henüz kurulmadı.

7 Eylül sohbet güncellemesiyle Ada’nın önceki yanıtı, aynı konuşmada Bora’ya
metni tekrar yapıştırmadan çevirtildi. Backend şema79 ile çalışıyor. Güncel
süreç kayıtları `/private/tmp/ortak-private-20260905/rollouts/employee79-d2b137c/current-owners79.json`
dosyasındadır. Employee kimliği güncellemesi `d2b137c` ile API ve native pakete
kuruldu. İmzalı canlı API kontrolü üç çalışanı doğru çözdü. Bilgisayarda genel
girdi sorunu gözlendiği için yeni etiketlerin native kabulü henüz tamamlanmadı;
son masaüstü süreci kapandı ve girdi sorunu giderilirken yeniden açılmadı.
Native durum kaydı aynı rollout dizinindeki `native-current-state.json` içindedir.

## Günlük kullanım

- **Office:** `ortak-private` kanalından konuşmaya başlayın. Belirli bir çalışana
  iş vermek için mesaj yazarken adını seçip etiketleyin. İsimsiz insan mesajlarında
  merkezi yönlendirici uygun çalışanları sınırlar içinde seçer; her mesajın yanıt
  alması gerekmez. Çalışanlar bağımsız olarak kanalı dinleyip birbirlerini uyandırmaz.
- **Employees:** çalışanın kalıcı kimliğini, durumunu ve çalışma ayarlarını görün.
  Hazırlanmış yapılandırmalar model/akıl yürütme ayarlarını değiştirebilir.
  Yeni revizyon kaydetmek çalışan kimliğini ya da geçmişini değiştirmez.
  Etkinleştirme, hazırlanan runtime, bellek ve imzalayıcı kontrollerinden geçer.
- **Activity:** neden çalıştırıldığını, işleyişi ve teslim durumunu izleyin.
  Yanıt üretiminin bitmesi ile Office'e teslim ayrı durumlardır. İptal düğmesi
  bir istek oluşturur; tamamlandığına ilişkin kayıt gelene kadar beklemede görünür.
- **Projects & Work:** proje ve görev oluşturun, çalışan atayın, çıktı ve
  dosyaları inceleyin. Çalışanın işi bitirmesi görevi incelemeye getirir;
  kabul ölçütlerini ve son tamamlamayı insan onaylar.
- **Memory:** paylaşılacak metni gözden geçirip onaylayın, ardından yayınlayın.
  **Stop using** gelecekteki kullanımı durdurur ve kaldırma işlemini izlenebilir
  biçimde kaydeder. Önceki kullanım geçmişi silinmez. Süresi dolmuş bir onay
  kendiliğinden yenilenmez.

Deniz için hazırlanmış şifreli doğrudan konuşma ayrı bir akıştır. Bu akışın
şifreli gönderim/teslim geçmişi yerel korumalı depoda tutulur. Genel Activity ve
normal çalışma günlükleri şifreli konuşma içeriğini taşımaz.

## ChatGPT hesabı ve model

Hermes, bu kurulumda tek bir açıkça paylaşılmış ChatGPT OAuth bağlantısını
kullanır. Ada, Bora, Deniz ve anlamsal yönlendirici bu bağlantıya bağlıdır.
Codex uygulamasında hesap değiştirmek Hermes bağlantısını otomatik değiştirmez.
Yeni hesap için Hermes'in tarayıcı girişini tamamlamak gerekir; anahtarı sohbete
yazmak veya Codex `auth.json` dosyasını kopyalamak gerekmez.

6 Eylül 2026 22:15 UTC'de yeni hesap kaydedildi ve Sol/high ile tek gerçek yanıt
başarıyla doğrulandı. Giriş container'ı kaldırıldı. Model seçimi çalışan
revizyonundadır; belirli bir model bu ürünün sabit bağımlılığı değildir.

## Durumu anlama

Arayüzde bağlantı kesilirse yeniden bağlanma/yenileme ve kimlik kurtarma yolları
kullanılabilir. Bir servisin portunun açık olması, bir çalışanın çalışabileceğini
kanıtlamaz; Employees ve Activity'deki yetkili sonuçlara bakın. Kullanıcı arayüzü
başka bir şirket adresine geçerek hatayı atlamaz.

Geliştirici için yalnızca dinleyici durumunu kontrol eden, model çağrısı yapmayan
komut aşağıdadır. Eski yardımcı bazı artefakt alanlarını tarihsel konumlarından
okur; bu alanları güncel dağıtım kanıtı olarak kullanmayın.

```sh
cd /Users/nambse/.codex/worktrees/14b1/ortak.dev
/Users/nambse/.pyenv/versions/3.12.8/bin/python3 scripts/ortak/private_status.py \
  --state-dir /private/tmp/ortak-private-20260905
```

## Depolama ve kurtarma

Bu makineye ait özel durum `/private/tmp/ortak-private-20260905` ve
`/private/tmp/ortak-hermes-v0-private-20260905` altındadır. Uygulamanın şifreli
konuşma deposu `~/Library/Application Support/dev.ortak.private20260905`
altındadır. Bunlar derleme önbelleği değildir; disk temizliği sırasında silmeyin.

Şema78 için temiz kurulum, eski şema44'ten yükseltme, dolu tam yedek, ayrı hedefe
fiziksel geri yükleme ve özgün hizmetleri yeniden başlatma doğrulandı. Bu aynı
makinede kurtarma kanıtıdır. Yedekten geri yüklenen runtime etkinleştirilmedi.
Son tam yedek hesap değişiminden önce alındığı için eski OAuth oturumunu içerir;
yeni hesabın üzerine doğrudan geri yüklenmemelidir.

Kullanılmayan, bu çalışmaya ait durdurulmuş yardımcı container ve image'lar
kimlikleri doğrulanarak temizlenir. Veri volume'ları, geri yükleme doğrulama
veritabanları ve çalışan image'lar genel `prune` komutuyla kaldırılmaz.

Güncel dağıtım ve tamamlanma kaydı:
[CONTINUATION_PROGRESS_2026-09-05.md](CONTINUATION_PROGRESS_2026-09-05.md).
Operatörün dondurulmuş kurtarma adımları:
[FULL_STACK_RECOVERY.md](../../runtime/private-stack/FULL_STACK_RECOVERY.md).
