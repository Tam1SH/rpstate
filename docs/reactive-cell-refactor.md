# Рефакторинг: `ReactiveCell` и единственный писатель у кэша

Статус: план. Ветка: `refactor/reactive-cell`. Целевая версия: 0.10.0 (ломающая).

## Зачем

Отправная точка — двойной фаер в `entry_signal`: одна запись поднимает подписчика дважды.
Причина не в подписках, а в том, что **у кэша два писателя**: локальная запись
(`inner.set(value, None)`) и круг через стор. Отсюда же выросли `sync_source`,
write-back подписка и весь анти-эхо код.

`Field` этой проблемы не имеет никогда: `field_set` пишет в бэкенд и **не трогает сигнал**
([field_ops.rs:8-26](../crates/core/amethystate-core/src/primitives/field_ops.rs)), а кэш
обновляется только из подписки стора через `field_apply_remote_value`. Один писатель — эха нет.

Второе, что вскрылось: `Field::as_signal()` и `MapEntrySignal::signal()` возвращают **один тип
с противоположной семантикой**. Первый односторонний (запись уходит в кэш и теряется), второй
двусторонний. Различить на уровне типов нельзя — это ловушка, и `entry_signal` со своей
write-подпиской был обходом именно её.

Вывод: убрать сырой `Signal` из публичного API и дать вместо него `ReactiveCell` —
конкретный тип-«валюту», который **всегда** пишет насквозь. Тогда двойной фаер становится
непредставимым, а не починенным, и `PartialEq`/дедуп не нужны.

## Целевая архитектура

```rust
#[derive(Clone)]
pub struct ReactiveCell<T> {
    cache: Signal<T>,                                        // get/subscribe — напрямую
    writer: Arc<dyn Fn(T) -> Result<(), WriteError> + Send + Sync>,
    keepalive: Option<Arc<dyn Send + Sync>>,                 // держит read-подписку
}
```

- `K`, `S`, `M` стираются в замыкание — ячейка кладётся в `HashMap<ID, ReactiveCell<u64>>`.
- `get()`/`subscribe()` идут прямо в кэш, диспетчеризации нет.
- Косвенный вызов только у `set()`, а записи идут в темпе пользователя, не кадра.
- `writer` держит путь записи, `keepalive` — путь чтения. Для `Field` совпадают
  (он хранит `store_sub`, и `Clone` его сохраняет), для entry-ячейки нет.

Публичная реактивная поверхность после рефакторинга:

| тип | что это | где значение |
|---|---|---|
| `Field` | ячейка | стор, либо кэш (volatile) |
| entry-ячейка мапы | ячейка по ключу | стор |
| `Pipeline` | производное, только чтение | кэш |
| `ReactiveCell` | стёртая ячейка, «валюта» | делегирует |

`Signal` уезжает в `pub(crate)`: он остаётся внутренним кэшем и диспетчером подписок.

### Перф — измерено, не гадаем

[`benches/cell_dispatch_bench.rs`](../crates/main/amethystate/benches/cell_dispatch_bench.rs),
медианы (ns):

| | `u64` | `String` | `Vec<u8;128>` |
|---|---|---|---|
| `Signal::get()` напрямую | 15.32 | 67.89 | 75.52 |
| `Arc<dyn>` | 15.66 | 69.95 | 77.98 |
| enum + match | 15.14 | 68.43 | 77.01 |
| **кэш впереди (выбранный)** | **15.27** | **67.27** | **72.03** |
| `Field::get()` настоящий | 15.22 | 67.06 | 73.00 |

`Field::get()` равен `Signal::get()` — обёртка инлайнится в ноль. Выбранная форма стоит
столько же, сколько прямой доступ. Доминирует клон `T` (15 → 68 ns), диспетчеризация — шум.
Вариант с enum отвергнут не по перфу, а потому что тащит `K`/`S`/`M` в тип и ломает стирание.

---

## Этапы

### 1. Один `WriteError`

Сейчас одна и та же ошибка продублирована четыре раза: generic в ядре
([error.rs:3-29](../crates/core/amethystate-core/src/primitives/error.rs)) и конкретная в main
([reactive/error.rs:4-62](../crates/main/amethystate/src/reactive/error.rs)) — плюс два почти
посимвольно совпадающих `From`-импла.

- [ ] `WriteError<E>` в ядре, `WriteError` (по `StorageError`) в main.
- [ ] `FieldError`/`ReactiveMapError` — алиасы на него.
- [ ] Схлопнуть два `From`-импла в один.

**Приёмка:** `cargo build --workspace`, ~60 строк дублей удалено.
**Зачем первым:** разблокирует сигнатуру `writer` в `ReactiveCell` без ассоциированного типа.

### 2. `Signal`: расщепить запись, починить гонку

- [ ] `set(value)` и `set_with_source(value, source: Uuid)` — по конвенции `Store`
      (`set`/`set_with_source` уже так).
- [ ] `emit` принимает уже сохранённый `Arc<T>`, а не перечитывает `load_full()`.

**Гонка:** сейчас `set` кладёт значение и зовёт `emit`, который **перечитывает** `ArcSwap`
([signal.rs:70-76](../crates/core/amethystate-core/src/primitives/signal.rs)). Между этим
другой поток успевает записать своё — подписчик получает чужое значение со своим `source`.
Провенанс несёт анти-эхо, так что рассинхрон может обернуться ложным write-back.

**Приёмка:** тест на конкурентные записи — `(value, source)` всегда согласованы.
Внешние изменения (`source: None`, [store.rs:771](../crates/main/amethystate/src/store/backend/text/store.rs))
проходят через `set` без `Option` в сигнатуре второго пути.

### 3. `ReactiveCell` + конструкторы

- [ ] Тип по схеме выше.
- [ ] `ReactiveCell::new(initial)` — volatile, закрывает дыру «in-memory значение без выдуманного `path`».
- [ ] `Field::cell()` — **только для `WritableMode`** (по существующей машинерии `AccessMode`).
      Writer захватывает клон `Field` → `me.set(v)`, значит `instance_id` и провенанс
      сохраняются даром ([field.rs:157](../crates/main/amethystate/src/reactive/field.rs)),
      `subscribe_external` и трейсинг продолжают работать через ячейку. `keepalive: None` —
      клон `Field` уже держит `store_sub`.

**Приёмка:** уронить исходный `Field`, у ячейки `get()` продолжает отдавать актуальное.

### 4. Entry-ячейка: store-first

- [ ] `MapEntrySignal` → `MapEntry` (после store-first это не «signal»).
- [ ] `set` пишет прямо в мапу (`map.set_or_create`), кэш обновляет **только** read-подписка.
- [ ] Выкинуть `sync_source`, write-back подписку, `.signal()`.
- [ ] `entry_cell(key, default) -> ReactiveCell<V>`, `keepalive: Some(read_sub)` —
      read-подписку больше никто не держит.
- [ ] `set` возвращает `Result` — сейчас ошибка проглатывается
      (`let _ = map_for_write.set_or_create(...)`,
      [entry_signal.rs:52](../crates/main/amethystate/src/reactive/entry_signal.rs)):
      интерцептор отклонил запись, а сигнал уже держит новое значение и молча врёт.

**Приёмка (тесты):**
- один фаер на запись, в обе стороны — правится
  [tests/entry_signal.rs](../crates/main/amethystate/tests/entry_signal.rs) (было 1 и 3, станет 1 и 2);
- интерцептор переписал значение → ячейка видит переписанное, не оптимистичное;
- интерцептор отклонил → `set` вернул `Err`, кэш не изменился;
- внешнее изменение файла по-прежнему доезжает;
- уронить владельца → `get()` не устаревает (инвариант `keepalive`).

### 5. `Signal` → `pub(crate)`

Только **после** миграции guinea (этап 8) — сейчас `guinea_core::signal::Signal` это алиас
на наш тип, и на нём стоит весь их реактивный слой.

- [ ] Убрать из ре-экспортов ([main lib.rs:23](../crates/main/amethystate/src/lib.rs),
      [core lib.rs:38](../crates/core/amethystate-core/src/lib.rs)).
- [ ] `Field::as_signal()` удалить — его роль забирает `Field::cell()`.

**Приёмка:** снаружи `Signal` недостижим; «одностороннюю» ячейку получить нельзя в принципе.

### 6. Фикс `*_external`

Фильтрация обрабатывает только `Update`; `Insert`/`Remove`/`Clear` едут мимо и срабатывают
на собственную запись хендла ([map.rs:121-145](../crates/main/amethystate/src/reactive/map.rs)).
`set_or_create` по новому ключу — это `Insert`, то есть дыра рабочая.

- [ ] Сравнивать `change.source()` одинаково для всех вариантов
      (он есть у всех, [change.rs:11-31](../crates/core/amethystate-core/src/change.rs)).

**Приёмка:** тест на `Insert`/`Remove`/`Clear` от своего хендла — `*_external` молчит.
Независимо от остальных этапов, можно делать в любой момент.

### 7. `&T` в подписках

Все реализации `Reactive<T>` уже сидят на `Signal`, который отдаёт `&T`, и **клонируют
только чтобы попасть в by-value сигнатуру трейта**
([field_core.rs:72-79](../crates/core/amethystate-core/src/primitives/field_core.rs),
[pipeline.rs:80](../crates/core/amethystate-core/src/primitives/pipeline.rs)).
`emit` держит `Arc<T>` весь цикл по подписчикам — ссылка корректна по построению.

- [ ] `Reactive::subscribe*` на `for<'a> Fn(&'a T, ...)`, убрать клоны в реализациях.
- [ ] Согласовать `ArenaReactive` ([arena/pipeline.rs:27-35](../crates/main/amethystate-arena/src/pipeline.rs)) — иначе два трейта разъедутся.

**Объём:** ~85 замыканий в репо, 3 в guinea. Делать последним, когда семантика устоялась.
**Проверить:** умеет ли арена отдать `&T` с нужным временем жизни — для `Signal`-путей доказано, для неё нет.

### 8. Миграция guinea

- [ ] `IntoSignal<T>` → `IntoCell<T>`; импл для `Field` теперь безопасен
      (`as_signal()` терял записи, `cell()` — нет).
- [ ] `TableLayout { widths: HashMap<ID, ReactiveCell<u64>> }`.
- [ ] `sig.set(w, None)` → `cell.set(w)?` — 7 мест.
- [ ] 3 замыкания подписок под `&T` (этап 7).

---

## Порядок

```
1 (ошибки) ──┬─> 3 (ReactiveCell) ──> 4 (entry store-first) ──> 8 (guinea) ──> 5 (Signal internal)
2 (Signal)  ─┘
6 (*_external) — независимо
7 (&T) — последним
```

## Открытые вопросы

- **Имя.** `ReactiveCell` — чтобы не путать со `std::cell::Cell`. Трейта не вводим:
  конкретного типа достаточно, `Pipeline` остаётся отдельным (только чтение).
- **Object-safety** больше не нужна: стирание делает замыкание в `writer`, а не `Arc<dyn Trait>`.
  Если позже понадобится `Box<dyn>`-гетерогенность — возвращаемся к `CellBackend`.
- **Арена** (`amethystate-arena`) идёт своим трейтом. Тащить ли её в `ReactiveCell` — решить
  после этапа 4, отдельно.
- **`update`/`modify`** сейчас скопированы в `Field` и `ReactiveMap`. Свести в `ReactiveCell`
  один раз, но они read-modify-write и неатомарны — задокументировать явно.
