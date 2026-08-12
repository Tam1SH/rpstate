# Аудит: ядро и основной крейт

Два прохода по `amethystate-core` и `amethystate`. Ничего из перечисленного не чинилось —
это список к разбору.

Пометки: **ПОЧИНЕНО** — исправлено, тест лежит в репозитории; **[видел сам]** — подтвердил
чтением кода; **[не проверял]** — со слов аудита.

Одна находка при проверке **не подтвердилась** — см. 11.

Не входит сюда то, что уже в работе: двойное уведомление на `clear` (этап 6.5 плана) и
флейки `file_watch_emits_*`.

---

## Потеря данных

### 1. Дебаунсер redb/sqlite теряет запись — ПОЧИНЕНО

[redb/mod.rs:156-195](../crates/main/amethystate/src/store/backend/redb/mod.rs),
[sqlite/mod.rs:373-421](../crates/main/amethystate/src/store/backend/sqlite/mod.rs)

Поток дебаунсера клонирует `pending`, **отпускает лок**, коммитит транзакцию, а затем удаляет
из `pending` все ключи своего снимка — по имени, не сверяя значение. `set`/`delete` берут
только `pending.lock()` и не берут `write_lock`, поэтому запись, попавшая в окно между
клонированием и удалением, выбрасывается из буфера и на диск не попадает.

```
set("k", v1)                 → pending["k"] = v1
  дебаунс сработал, changes = {k: v1}, лок отпущен
set("k", v2)                 → pending["k"] = v2, подписчики уведомлены, UI показывает v2
  дебаунсер коммитит v1
  дебаунсер: lock.remove("k") ← выбрасывает v2
```

На диске v1, в буфере пусто, v2 потеряна молча. Срабатывает при обычном вводе с частотой
около `save_debounce`. Лечится сверкой значения при удалении либо удержанием `write_lock` в `set`.

### 2. `flush_prefix` теряет буфер при ошибке записи — ПОЧИНЕНО

[redb/mod.rs:67-91](../crates/main/amethystate/src/store/backend/redb/mod.rs),
[sqlite/mod.rs:45-77](../crates/main/amethystate/src/store/backend/sqlite/mod.rs)

Изменения **вынимаются** из `pending` до открытия транзакции, любой `?` ниже уничтожает их —
ни в памяти, ни на диске. `save_now` → `Drop` глушит ошибку через `let _ =`, так что при
закрытии приложения со сломанным файлом БД весь несохранённый буфер исчезает без следа.

Поток дебаунсера ведёт себя **противоположно** — специально сохраняет буфер при неудаче
(есть тест `test_debouncer_retains_buffer_on_simulated_transaction_failure`). Один и тот же
сбой даёт разный исход в зависимости от того, кто писал.

### 3. Гонка `has_pending` в text-бэкенде откатывает свежую запись — ПОЧИНЕНО (репро нет)

[text/store.rs:249-253](../crates/main/amethystate/src/store/backend/text/store.rs) против
[464-478](../crates/main/amethystate/src/store/backend/text/store.rs)

Проверка `has_pending` и мутация документа не атомарны:

```
вотчер:      has_pending == false → входит в sync_external_changes, читает файл (старое)
прикладной:  set_node мутирует doc, has_pending = true
вотчер:      берёт doc.write(), видит расхождение → ЗАТИРАЕТ свежее значение диском
             и рассылает подписчикам откат
```

Второе окно того же рода в `debouncer` ([store.rs:230-235](../crates/main/amethystate/src/store/backend/text/store.rs)):
`persist()` сериализует снимок, а `has_pending.store(false)` идёт позже — запись между ними
помечается сохранённой, не будучи записанной.

`AtomicBool` здесь принципиально недостаточен: нужен счётчик поколений документа либо проверка
под тем же `doc.write()`.

---

## Дедлоки и рассогласование

### 4. `ReactiveMapCore::notify` держит мьютексы во время колбэков — ПОЧИНЕНО

[map_core.rs:268-297](../crates/core/amethystate-core/src/primitives/map_core.rs)

`subscribers_key.lock()` и `subscribers_any.lock()` живут весь цикл `cb(change)`.
`std::sync::Mutex` нереентерабелен, а колбэк внутри колбэка — штатный сценарий.

Проба: подписчик на мапу, который в ответ пишет в ту же мапу («отреагировать на изменение
записью») — **дедлок за 5 секунд**. Плюс паника в колбэке отравляет мьютекс: после этого
`subscribe_*` паникуют навсегда, а `notify` со своим `if let Ok` молча перестаёт уведомлять.

`Signal::emit` ([signal.rs:106-111](../crates/core/amethystate-core/src/primitives/signal.rs))
специально копирует список до вызова. `notify` надо привести к той же схеме.

### 5. Field игнорирует `Delete` — ПОЧИНЕНО

[primitives_factory.rs:61-68](../crates/main/amethystate/src/store/primitives_factory.rs)

Подписка поля обрабатывает только `event.new.is_some()`. После `store.delete(path)` или внешнего
удаления ключа из файла сигнал продолжает отдавать старое значение, тогда как `store.get` даёт
`None`, а перезапуск — `default`. Одинаково на всех бэкендах. Ожидаемое поведение нигде не задано.

---

## Async-путь разъехался с sync

Все три — [map_ops_async.rs](../crates/core/amethystate-core/src/primitives/map_ops_async.rs),
две из них я видел сам по ходу работы над entry-ячейкой.

### 6. Async теряет provenance **[видел сам]**

Строки 213/216/222: пишет через `backend.set`/`delete` **без source**, тогда как sync передаёт
`processed.source()`. Собственная запись возвращается как «внешняя», `subscribe_any_external`
её не отфильтрует — ровно тот класс эха, который мы вычистили из entry-ячейки.
При этом `AmeBackendAsync` предоставляет `set_with_source`/`delete_with_source`.

### 7. Async обновляет кэш **до** записи в бэкенд **[видел сам]**

Строка 204 против sync на [map_ops.rs:228](../crates/core/amethystate-core/src/primitives/map_ops.rs).
Ошибка ввода-вывода → кэш уже очищен/перезаписан, на диске старое, уведомления об откате нет.
`values()`/`get_sync` читают только кэш и отдают то, чего в хранилище нет. Для `Clear` ошибка
в середине цикла оставляет частично удалённый бэкенд плюс полностью очищенный кэш.

### 8. У async нет `notify_after_commit` **[не проверял]**

Строка 227 всегда зовёт `core.notify`, тогда как sync получает флаг и передаёт `false` для
set/remove. Если async-бэкенд тоже эмитит через `subscribe_map`, каждая запись доходит дважды.

---

## Тихо неверное поведение

### 9. Превышение глубины интерцепторов пропускает изменение мимо валидации — ПОЧИНЕНО

[intercept.rs:11-27](../crates/core/amethystate-core/src/primitives/intercept.rs),
[map_core.rs:235](../crates/core/amethystate-core/src/primitives/map_core.rs),
[field_core.rs:111](../crates/core/amethystate-core/src/primitives/field_core.rs)

При `depth >= 10` `enter` возвращает `None`, блок пропускается, `run_interceptors` возвращает
`Ok(исходное)` — непроверенное. Валидатор, отбрасывающий значения вне диапазона, на 11-й
вложенной записи пропускает мусор в бэкенд. Должно быть `Err`, а не тихое «ок».

### 10. `old` в событии `Set` у redb/sqlite берётся только из буфера — ПОЧИНЕНО

[redb/mod.rs:313-316](../crates/main/amethystate/src/store/backend/redb/mod.rs),
[sqlite/mod.rs:167-170](../crates/main/amethystate/src/store/backend/sqlite/mod.rs)

После флеша ключ исчезает из буфера, и следующая запись эмитит `Set` с `old: None`, хотя
значение есть на диске. `delete_with_source` при этом ходит за старым значением в БД корректно,
а text-бэкенд всегда отдаёт настоящий `old`. Следствие: подписчик мапы получает
`MapChange::Update { old_value: V::default() }` — сравнение старого с новым ломается на
redb/sqlite и работает на text.

```
set("m.k", 5) → подождать save_debounce → set("m.k", 7)
  приходит Update { old_value: 0, new_value: 7 }
```

### 11. `remove` определяет существование ключа по кэшу — НЕ ПОДТВЕРДИЛОСЬ для sync

[map_ops.rs:144-147](../crates/core/amethystate-core/src/primitives/map_ops.rs)

**Для sync-пути неверно.** `reactive_map_with_scope_key` засевает кэш из `store.scan_prefix`
при конструировании ([primitives_factory.rs:130-139](../crates/main/amethystate/src/store/primitives_factory.rs)),
так что ключ от предыдущего запуска в кэше есть и `remove` его находит. Тест
`remove_finds_a_key_written_by_an_earlier_run` это закрепляет.

Находка живёт в **async**: `new_with_backend_and_id` ставит кэш равным `initial_values` и
ничего не сканирует ([async_impl/map.rs:106-109](../crates/core/amethystate-core/src/async_impl/map.rs)).
Там утверждение ниже, скорее всего, верно, но нужен async-харнесс.

Исходная формулировка аудита: кэш заполняется только `initial_values` и последующими
изменениями; ключ, записанный предыдущим
запуском, при пустом `initial_values` не удалится — `remove` вернёт `Ok(None)`. Инвариант
«кэш = бэкенд» нигде не проверяется и после старта не выполняется.

### 12. Для `Clear` прогоняются key-интерцепторы всех ключей — ПОЧИНЕНО

[map_core.rs:240-255](../crates/core/amethystate-core/src/primitives/map_core.rs)

При `change.key() == None` берутся все ключи из `interceptors_key`, каждому скармливается один
и тот же `MapChange::Clear`, результат накапливается — интерцептор, вернувший `Insert`,
превращает `Clear` в `Insert` для остальных. Порядок обхода `HashMap` недетерминирован.

---

## Миграции и инициализация

### 15. У мапы нет порядка — ПОЧИНЕНО

Не из отчётов аудита — всплыло при разборе пункта 12.

`entries()` берёт порядок из `backend.scan_prefix`, а тот склеивает закоммиченные ключи с
несохранённым буфером. Буфер — `HashMap`, поэтому до сброса ключи выходят в порядке хеширования,
**разном на каждом запуске**, а после сброса — отсортированными.

Вставка `[zulu, alpha, mike, bravo, delta]`, три запуска подряд:

| бэкенд | до `save_now` | после |
|---|---|---|
| redb | `[bravo, delta, alpha, mike, zulu]` → `[alpha, zulu, mike, bravo, delta]` → `[alpha, mike, delta, zulu, bravo]` | `[alpha, bravo, delta, mike, zulu]` |
| sqlite | так же случайно | `[alpha, bravo, delta, mike, zulu]` |
| json | `[zulu, alpha, mike, bravo, delta]` (вставки) | то же |
| toml | `[zulu, alpha, mike, bravo, delta]` (вставки) | то же |
| ron | `[alpha, bravo, delta, mike, zulu]` | то же |

Следствие: на redb/sqlite вью со списком ключей **переставляет себя сам** в момент срабатывания
дебаунса, и по-разному на каждом запуске. Плюс три разных порядка между бэкендами при том, что
контракт нигде не заявлен.

Сюда же ложится пункт 12: `interceptors_key` тоже `HashMap`, отсюда недетерминированный порядок
обхода интерцепторов.

**Починено:** `scan_prefix` сортирует склеенный результат во всех трёх бэкендах, контракт
заявлен на `Store::scan_prefix` и в доке проекта. Заодно `entries()` стал ленивым, появился
`keys()` без десериализации, а async `values()`/`entries()` перестали схлопывать порядок в
`HashMap`. Порядок вставки не выбран сознательно: он
нигде не хранится, json/toml дают его случайно из структуры документа, а восстановить его для
redb/sqlite было бы нечем. Сортировка выводится из самих ключей и ничего лишнего не требует.

Тесты в [tests/map_order.rs](../crates/main/amethystate/tests/map_order.rs), прогнаны на всех
пяти конфигурациях (redb, json, toml, ron, sqlite).

### 13. Версия не двигается, если под неё нет шага **[не проверял]**

[migration/engine.rs:259-269](../crates/main/amethystate/src/migration/engine.rs)

`meta` пишется только при `!applied_steps.is_empty()`. Если целевая версия поднята, а шага под
неё в плане нет, не применяется ничего и ошибки `Gap` тоже нет. Итог: `set_meta` не вызван,
`component_needs_work` возвращает `true` на каждом старте, снимок схемы не пишется, а отчёт
рапортует `Committed { steps: [] }` — успех.

Рядом [engine.rs:107](../crates/main/amethystate/src/migration/engine.rs): `ensure_snapshots()`
выполняется даже после `ComponentOutcome::Failed` и перезаписывает снимок схемы новой схемой
кода при неперемигрированных данных — `calculate_drift` на следующем запуске расхождения уже
не увидит, диагностика по упавшему префиксу теряется навсегда.

### 14. `is_initialized` привязан к scope, а не к пути мапы **[не проверял]**

[primitives_factory.rs:141](../crates/main/amethystate/src/store/primitives_factory.rs)

Дефолты мапы засеваются по флагу `scope_key` (`StateScope::PREFIX`), который выставляется один
раз на весь scope. Вторая мапа, добавленная в существующую структуру, у проинициализированных
пользователей не получит свои `defaults` никогда — окажется пустой, без признака ошибки.
