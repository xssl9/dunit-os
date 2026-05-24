# 🔮 Filesystem

**Статус:** 🔮 Будущее  
**Из плана:** [[../../ROADMAP|ROADMAP]] → Task 9  
**Требования:** [[../../Origin/REQUIREMENTS|REQ-10]]  
**Зависит от:** [[../InProgress/Drivers|Disk Driver (ATA/AHCI)]]

---

## Чеклист

- [ ] ext2/ext3 поддержка
- [ ] FAT32 поддержка
- [ ] File permissions
- [ ] Symbolic links
- [ ] Mount/unmount

## VFS структура (из design doc)

```
/
├── dev/   (fb0, kbd, mouse)
├── bin/   (init, netsurf, terminal)
└── proc/  (1/, 2/, ...)
```

## Скриншоты

> Место для скринов

## Заметки

_Место для заметок_
