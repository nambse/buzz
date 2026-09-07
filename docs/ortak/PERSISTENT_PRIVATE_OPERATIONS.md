# Ortak özel kurulumunu çalıştırma

Bu komutlar yalnız bu Mac'teki mevcut `ortak-private-v0` kurulumunu yönetir.
Kalıcı dizin: `~/.local/share/ortak/private-v0`. Komutlar yeni çalışan, kimlik,
OAuth bağlantısı, container, image veya volume oluşturmaz.

```sh
~/.local/share/ortak/private-v0/bin/ortak status
~/.local/share/ortak/private-v0/bin/ortak start
~/.local/share/ortak/private-v0/bin/ortak stop
~/.local/share/ortak/private-v0/bin/ortak restart
```

Docker Desktop açık olmalıdır. `start` mevcut altı container'ı ve dört kullanıcı
servisini açar. `stop` önce yeni istek girişini kapatır, en fazla45 saniye bekleyen
işlerin bitmesini bekler, sonra çalışan süreçleri ve veri servislerini kapatır.
Verileri silmez. İşler bitmezse worker ve veri servisleri açık kalır;
`lifecycle/operation.json` bunun kaydını tutar. `start` ile arayüz erişimini açıp
Activity'den işi inceleyebilir veya iptal edebilirsiniz. Daha sonra `stop` tekrar
denenebilir. Manuel kapanan servisler sonraki oturum açılışında da kapalı kalır;
`start` bu tercihi geri açar.

`status` sabit container kimliği, image, disk bağlantıları, ağ adları ve yerel
portları kayıtla karşılaştırır. Native binary ve kurulu launcher dosyalarını
SHA256 ile doğrular. Servis işlemlerini, bekleyen kayıt sayılarını ve HTTP yanıt
durumlarını gösterir. HTTP401/403 kimlik doğrulamasının gerektiğini gösterir;
geçerli model bağlantısı veya çalışan sağlığı kanıtı sayılmaz. Status token
yüklemez ve gerçek model çağrısı yapmaz.

Native uygulama servislerden ayrı kalır. Kısa kesintide son başarılı Work
görünümünü koruyup yazmayı durdurur. Bağlantı geldikten sonra Employees **Refresh**,
Work **Refresh work**, Activity **Reload timeline** ile yeniden bağlanılabilir.
Mevcut `b49c04a` pakette Work bağlantı/bağımlılık hataları için ayrıca **Retry work
links** / **Retry dependencies** kullanılır. Son kaynak düzeltmesi **Refresh
work** eylemini bu iki panele de bağlar; henüz yeni native pakete alınmadı.

Kurulu launcher kendi Python dosyalarını kalıcı `lifecycle/code-…` dizininden
okur; çalışması repo dosyalarının yerinde olmasına bağlı değildir. Servis başına
`logs/{relay,api,worker,management}.log` en fazla4 MiB, üç döndürülmüş kopya tutar.
Önceki bootstrap günlükleri korunur. Çalışan servisler `dev.ortak.private-v0.*`
LaunchAgent'larıdır. Bu düzen, Docker Desktop'ın açıldığı bir kullanıcı oturumunu
gerektirir; FileVault kilidini açmaz veya Mac'i kendiliğinden başlatmaz.

İlk kayıt ve launcher kodu güncellemesi, yetkili checkout'tan seçili Python ile:

```sh
/Users/nambse/.pyenv/versions/3.12.8/bin/python3 scripts/ortak/private_stack.py install
/Users/nambse/.pyenv/versions/3.12.8/bin/python3 scripts/ortak/private_stack.py upgrade-launcher
~/.local/share/ortak/private-v0/bin/ortak restart
```

`install` mevcut kaydı tekrar kullanır. `upgrade-launcher` önce eski kaynak
sahipliğini doğrular, yalnız işletim kodunu dondurur ve önceki seçimi saklar.
Veritabanı şemasını, native binary veya Hermes image seçimini değiştirmez.
Bu komut genel ürün yükseltmesi veya yedekten geri yükleme aracı değildir.

Tutarlı yedek almak için önce Ortak Private uygulamasından **Quit** ile çıkın.
Ardından kurulu araç mevcut kaynakları kapatır, yedeği alır ve servisleri yeniden
açar. `--native-bundle` ile o anda kullanılan özel uygulama paketini seçin:

```sh
~/.local/share/ortak/private-v0/bin/ortak backup --native-bundle '/tam/yol/Ortak Private.app'
~/.local/share/ortak/private-v0/bin/ortak verify-backup --backup ~/.local/share/ortak/private-v0/backups/YEDEK_KIMLIGI
```

`backup` çıktısındaki `captured_not_restored`, henüz geri yükleme kanıtı değildir.
`verify-backup` yeni, ayrı ve ağsız hedeflerde gerçek dosya/DB geri yüklemesi yapar;
source volume'ları veya aktif yapılandırmayı değiştirmez. Doğrulanmış hedefler
korunur ve veritabanları doğrulama bitince kapatılır. Bu bir aktif kurulumun
yerine otomatik geçiş komutu değildir: klonlanmış worker/controller sahipliğini
yeniden bağlayıp devreye alma, doğrulanmış ayrı bir operatör adımı gerektirir.
Yedek özel kimlik/OAuth verilerini de içerir; tamamı yerel0700/0600 dosyalardadır.
Arşivleri veya içeriklerini Git'e eklemeyin.

Gerçek schema80 yedeği `backups/8bfe40a32533441bad311de811fd24e9` altında alındı.
`recovery-verifications/12b55f9d120c4d25b95ea5ca0ec1dd35/receipt.json` gerçek
geri yükleme sonucudur. Ana DB156 tablo, Honcho26 tablo için bütün mantıksal
satır hash'leri, katalog ve sequence değerleri eşleşti. Beş soğuk volume'un dosya
hash/mod/uid/gid/nanosaniye zamanları ve MinIO metadata'sı eşleşti. Image gzip
footer/hash denetimi ve gerçek Docker image load geçti. Özel yapılandırma/app
verisi ve native paketler ayrı, etkinleştirilmemiş dizinlere açılıp doğrulandı.
Hermes journal integrity/foreign-key kontrolleri geçti: 17 kayıtlı run içinde
yedi profile probe bulunuyor; kullanıcı/Work tarafındaki gerçek run sayısı10.
SQLite WAL/SHM incelemesi, kaynak volume'a yazmadan sınırlı geçici kopyada yapılır.

Gerçek kabul: altı container ve dört native servis bu komutla kapanıp aynı
kimliklerle tekrar açıldı. On run, iki artifact ve tamamlanmış Work korundu;
iki Work bağlamının hash'leri değişmedi. Native Work/Activity yeniden bağlandı.
Kanıtlar kalıcı `evidence/lifecycle-{restart-02.log,status-after-restart.json,
native-restart-acceptance.json}` dosyalarındadır. Süreç ağacı, günlük sınırı,
sahiplik ve bekleyen iş testleri12/12 geçti. Arşiv, kapsam ve WAL denetimleriyle
genişletilen toplam22 test geçti. Bu sonuçlar tam Mac reboot veya farklı makine
kurtarması değildir; bu sınırlar ayrıca doğrulanmalıdır.
