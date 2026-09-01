---
title: Что несёт в себе ошибка
sidebar:
  label: Ошибки
  order: 20
---

Всякий вызов, способный упасть, отвечает в этой библиотеке `Report`'ом из
[`error_stack`](https://docs.rs/error-stack). Частей у него две, и enum из них
только первая:

- **цепочка контекстов** — что упало, на каждом уровне, который знал, что
  падает;
- **вложения** — подробности, которые несут типами, а не предложениями.

Следствие, которое надо знать прежде всего: кто печатает ошибку через `{}`,
тот видит верхнее предложение, а остальное выбрасывает.

## Вершина называет операцию

Карта, под которой лежит запись, не декодируемая в её тип значения, не
откроется вовсе. На этом отказе и разобрана вся остальная страница:

<!-- shown: what a failure says it is -->
```rust
let refused = store.kv().map::<String, u64>("labels").unwrap_err();

let context = refused.current_context();
let sentence = refused.to_string();
```
<!-- /shown -->

`context` здесь — `WriteError::Storage`, а `sentence` — *the store could not
carry out the write*.

`current_context()` отдаёт самый внешний контекст. Варианты называют
**операцию, которая упала**, а не то, обо что она упала: `StorageError::Write`,
`StorageError::Scan`, `StorageError::Codec`. Два движка, не сумевшие записать,
дадут один и тот же контекст, а различат их кадры под ним.

Так задумано. Тому, кто решает, что делать дальше, важно, легла ли запись, а не
кто сказал «нет» — redb или `serde_json`. Матчинг по движку привязал бы его к
тому, какой движок настроен.

Данные вариант несёт только там, где они **и есть** ошибка, а не её
обстоятельства. `WriteError::KeyNotFound(key)` называет ключ, потому что больше
в отчёте назвать его некому; `WriteError::SchemaOwned { path, declared }`
называет оба места, потому что столкновение — между ними.

Оба типа контекста сравнимы, так что проверить, какой у вас, хватит `==`.
`matches!` берут для вариантов с данными, когда спрашивают про сам вариант, а
не про его содержимое.

## Подробности приложены типами

Каждый факт — отдельный тип, поэтому разбирать сообщение обратно не нужно.
Живут они в `amethystate::errors::facts`: ключ, префикс, под которым шёл обход,
запись, на которой он споткнулся, файл, который он читал, размер значения.
Каждый — newtype над тем, что держит, а подпись к нему живёт в его `Display`.

`facts::all::<T, _>` отдаёт все факты одного типа, начиная с самого
внутреннего:

<!-- shown: reaching the entry that failed -->
```rust
let refused = store.kv().map::<u16, u64>("ports").unwrap_err();

let entries: Vec<&Entry> = facts::all::<Entry, _>(&refused).collect();
let prefixes: Vec<&Prefix> = facts::all::<Prefix, _>(&refused).collect();
```
<!-- /shown -->

В `entries` один `Entry("http")`, в `prefixes` — один `Prefix("ports")`. Карта,
не открывшаяся из-за одной плохой записи, называет, из-за какой именно, — и это
ровно та часть, которую печать через `{}` выбрасывает.

Спросите факт, которого в отчёте нет, — не получите ничего:

<!-- shown: asking for a fact the report does not carry -->
```rust
let refused = store.kv().map::<u16, u64>("ports").unwrap_err();

let key = facts::all::<Key, _>(&refused).next();
```
<!-- /shown -->

`key` здесь `None`: этот отчёт про один ключ и не был.

Какие факты в отчёте окажутся, зависит от того, кто был на стеке: прикладывает
их тот, кто их знал, и `Key` не приложит код, видевший один только префикс.
Читайте их как улики, которые есть тогда, когда есть, а не как схему.

Прикладывают лениво: на пути, который проходит успешно, не строится ничего.

## Как его напечатать

`{}` даёт верхний контекст и больше ничего. `{:?}` — всё целиком: каждый
контекст цепочки, а факты под тем кадром, который их приложил. Три примера ниже
настоящие: их напечатал прогон, который эту страницу и наполняет.

<!-- shown: an entry that will not decode -->
```rust
store.set(["ports", "http"], &1u64)?;

let undecodable = store.kv().map::<u16, u64>("ports").unwrap_err();
```
<!-- /shown -->

<!-- printed: an entry that will not decode from book_errors -->
```
the store could not carry out the write
├╴prefix: ports
│
╰─▶ the value could not be encoded or decoded
    ├╴prefix: ports
    ├╴entry: http
    ╰╴key type: u16
```
<!-- /printed -->

<!-- shown: a name that cannot be a level -->
```rust
let empty_level = store.set([""], &1u32).unwrap_err();
```
<!-- /shown -->

<!-- printed: a name that cannot be a level from book_errors -->
```
a name that cannot be a level
│
╰─▶ level 0 of the path has no name
```
<!-- /printed -->

<!-- shown: a path past the cap it was given -->
```rust
let shallow = StoreBuilder::new(settings)
    .limits(|l| l.key_depth(4))
    .build()?;

let too_deep = shallow.set(["a", "b", "c", "d", "e"], &1u32).unwrap_err();
```
<!-- /shown -->

<!-- printed: a path past the cap it was given from book_errors -->
```
deeper than this store reads back
├╴key: a.b.c.d.e
├╴levels: 5, and the limit is 4
├╴set by: limits(|l| l.key_depth(..))
╰╴what is stored here spends the same budget - this store reads 512 levels in all
```
<!-- /printed -->

Последний — та форма, к которой стоит тянуться, когда отказ пишете вы:
предложение называет, в чём отказали, а факты под ним отвечают на вопрос,
который читатель вот-вот задаст, — чей это был предел и во что обошёлся.

Отсюда правило: в лог идёт `{:?}`, а `{}` остаётся для той одной строки,
которую прочтёт человек. Отчёт, дошедший до него только через `{}`, потерял
как раз ту часть, что говорит, куда смотреть.

## Как отдать его коду, которому нужна ошибка `std`

<!-- shown: handing a report to something that wants a std error -->
```rust
fn writing(store: &amethystate::Store) -> Result<(), Box<dyn Error + Send + Sync>> {
    store.set(["ui", "width"], &800u32)?;
    Ok(())
}
```
<!-- /shown -->

`?` переносит отчёт в `Box<dyn Error + Send + Sync>` — этого хватает, чтобы
отдать его прямо в тест или в `main`, без прослойки.

Но сам `Report` — не `std::error::Error`, поэтому туда, где этот трейт стоит в
границах, он не пройдёт: `anyhow::Error` требует именно такую ошибку, чтобы её
обернуть, и так же устроено немало кода, написанного до этого стиля. Переход
делает `into_error`, и он ничего не теряет:

<!-- shown: turning a report into a std error -->
```rust
let std_error = refused.into_error();

let sentence = std_error.to_string();
let whole = format!("{std_error:?}");
```
<!-- /shown -->

`sentence` — то же самое *the store could not carry out the write*, а `whole`
по-прежнему держит `entry: http`: обёртка держит отчёт за собой, а не
сплющивает, поэтому на переходе не теряется ничего. `as_error` — тот же приём,
но взаймы: отдать отчёт, не отдавая насовсем.

`error_stack` реэкспортирован как `amethystate::error_stack`, а `Report` лежит
ещё и в `amethystate::errors`. Поэтому назвать его в своей сигнатуре не стоит
вам отдельной зависимости и не разъедется с этим крейтом по версии.
