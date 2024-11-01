#![cfg_attr(not(feature = "std"), no_std)]

//! A proc macro for designing loosely coupled Rust applications.
//!
//! [`entrait`](entrait) is used to generate an _implemented trait_ from the definition of regular functions.
//! The emergent pattern that results from its use enable the following things:
//! * Zero-cost loose coupling and inversion of control
//! * Dependency graph as a compile time concept
//! * Mock library integrations
//! * Clean, readable, boilerplate-free code
//!
//! The resulting pattern is referred to as [the entrait pattern](https://audunhalland.github.io/blog/entrait-pattern/) (see also: [philosophy](#philosophy)).
//!
//! # Introduction
//!
//! The macro looks like this:
//!
//! ```rust
//! # use entrait::entrait;
//! #[entrait(MyFunction)]
//! fn my_function<D>(deps: &D) {
//! }
//! ```
//!
//! which generates a new single-method trait named `MyFunction`, with the method signature derived from the original function.
//! Entrait is a pure append-only macro: It will never alter the syntax of your function.
//! The new language items it generates will appear below the function.
//!
//! In the first example, `my_function` has a single parameter called `deps` which is generic over a type `D`, and represents dependencies injected into the function.
//! The dependency parameter is always the first parameter, which is analogous to the `&self` parameter of the generated trait method.
//!
//! To add a dependency, we just introduce a trait bound, now expressable as `impl Trait`.
//! This is demonstrated by looking at one function calling another:
//!
//! ```rust
//! # use entrait::entrait;
//! #[entrait(Foo)]
//! fn foo(deps: &impl Bar) {
//!     println!("{}", deps.bar(42));
//! }
//!
//! #[entrait(Bar)]
//! fn bar<D>(deps: &D, n: i32) -> String {
//!     format!("You passed {n}")
//! }
//! ```
//!
//!
//! ### Multiple dependencies
//! Other frameworks might represent multiple dependencies by having one value for each one, but entrait represents all dependencies _within the same value_.
//! When the dependency parameter is generic, its trait bounds specifiy what methods we expect to be callable inside the function.
//!
//! Multiple bounds can be expressed using the `&(impl A + B)` syntax.
//!
//! The single-value dependency design means that it is always the same reference that is passed around everywhere.
//! But a reference to what, exactly?
//! This is what we have managed to abstract away, which is the [whole point](#testing).
//!
//!
//!
//! ### Runtime and implementation
//! When we want to compile a working application, we need an actual type to inject into the various entrait entrypoints.
//! Two things will be important:
//!
//! * All trait bounds used deeper in the graph will implicitly "bubble up" to the entrypoint level, so the type we eventually use will need to implement all those traits in order to type check.
//! * The implementations of these traits need to do the correct thing: Actually call the entraited function, so that the dependency graph is turned into an actual _call graph_.
//!
//! Entrait generates _implemented traits_, and the type to use for linking it all together is [`Impl<T>`](crate::Impl):
//!
//! ```rust
//! # use entrait::*;
//! #[entrait(Foo)]
//! fn foo(deps: &impl Bar) -> i32 {
//!     deps.bar()
//! }
//!
//! #[entrait(Bar)]
//! fn bar(_deps: &impl std::any::Any) -> i32 {
//!     42
//! }
//!
//! let app = Impl::new(());
//! assert_eq!(42, app.foo());
//! ```
//!
//! <details>
//! <summary>🔬 <strong>Inspect the generated code</strong> 🔬</summary>
//!
//! The linking happens in the generated impl block for `Impl<T>`, putting the entire impl under a where clause derived from the original dependency bounds:
//!
//! ```rust
//! # use ::entrait::Impl;
//! # trait Foo { fn foo(&self) -> i32; }
//! # trait Bar { fn bar(&self) -> i32; }
//! # fn foo(deps: &impl Bar) -> i32 { deps.bar() }
//! impl<T: Sync> Foo for Impl<T> where Self: Bar {
//!     fn foo(&self) -> i32 {
//!         foo(self) // <---- calls your function
//!     }
//! }
//! ```
//! </details>
//!
//! `Impl` is generic, so we can put whatever type we want into it.
//! Normally this would be some type that represents the global state/configuration of the running application.
//! But if dependencies can only be traits, and we always abstract away this type, how can this state ever be accessed?
//!
//!
//!
//! ### Concrete dependencies
//! So far we have only seen generic trait-based dependencies, but the dependency can also be a _concrete type_:
//!
//! ```rust
//! # use entrait::*;
//! struct Config(i32);
//!
//! #[entrait(UseTheConfig)]
//! fn use_the_config(config: &Config) -> i32 {
//!     config.0
//! }
//!
//! #[entrait(DoubleIt)]
//! fn double_it(deps: &impl UseTheConfig) -> i32 {
//!     deps.use_the_config() * 2
//! }
//!
//! assert_eq!(42, Impl::new(Config(21)).double_it());
//! ```
//!
//! The parameter of `use_the_config` is in the first position, so it represents the dependency.
//!
//! We will notice two interesting things:
//! * Functions that depend on `UseTheConfig`, either directly or indirectly, now have only one valid dependency type: `Impl<Config>`<sup>[1](#case-1-concrete-leaf-dependencies)</sup>.
//! * Inside `use_the_config`, we have a `&Config` reference instead of `&Impl<Config>`. This means we cannot call other entraited functions, because they are not implemented for `Config`.
//!
//! The last point means that a concrete dependency is the end of the line, a leaf in the dependency graph.
//!
//! Typically, functions with a concrete dependency should be kept small and avoid extensive business logic.
//! They ideally function as accessors, providing a loosely coupled abstraction layer over concrete application state.
//!
//!
//! ### Module support
//! To reduce the number of generated traits, entrait can be used as a `mod` attribute.
//! When used in this mode, the macro will look for non-private functions directly within the module scope, to be represented as methods on the resulting trait.
//! This mode works mostly identically to the standalone function mode.
//!
//! ```rust
//! # mod example {
//! # use entrait::*;
//! # #[entrait(SomeTrait)]
//! # fn some_trait<D>(_: &D) {}
//! # #[entrait(OtherTrait)]
//! # fn other_trait<D>(_: &D) {}
//! #[entrait(pub MyModule)]
//! mod my_module {
//!     pub fn foo(deps: &impl super::SomeTrait) {}
//!     pub fn bar(deps: &impl super::OtherTrait) {}
//! }
//! # }
//! ```
//! This example generates a `MyModule` trait containing the methods `foo` and `bar`.
//!
//!
//! # Testing
//! ## Trait mocking with `Unimock`
//!
//! The whole point of entrait is to provide inversion of control, so that alternative dependency implementations can be used when unit testing function bodies.
//! While test code can contain manual trait implementations, the most ergonomic way to test is to use a mocking library, which provides more features with less code.
//!
//! Entrait works best together with [unimock](https://docs.rs/unimock/latest/unimock/), as these two crates have been designed from the start with each other in mind.
//!
//! Unimock exports a single mock struct which can be passed as argument to every function that accept a generic `deps` parameter
//!   (given that entrait is used with unimock support everywhere).
//! To enable mock configuration of entraited functions, supply the `mock_api` option, e.g. `mock_api=TraitMock` if the name of the trait is `Trait`.
//! This works the same way for entraited modules, only that those already _have_ a module to export from.
//!
//! Unimock support for entrait is enabled by passing the `unimock` option to entrait (`#[entrait(Foo, unimock)]`), or turning on the `unimock` _feature_, which makes all entraited functions mockable, even in upstream crates (as long as `mock_api` is provided.).
//!
//! ```rust
//! # use entrait::entrait_export as entrait;
//! # use unimock::*;
//! #[entrait(Foo, mock_api=FooMock)]
//! fn foo<D>(_: &D) -> i32 {
//!     unimplemented!()
//! }
//! #[entrait(MyMod, mock_api=mock)]
//! mod my_mod {
//!     pub fn bar<D>(_: &D) -> i32 {
//!         unimplemented!()
//!     }
//! }
//!
//! fn my_func(deps: &(impl Foo + MyMod)) -> i32 {
//!     deps.foo() + deps.bar()
//! }
//!
//! let mocked_deps = Unimock::new((
//!     FooMock.each_call(matching!()).returns(40),
//!     my_mod::mock::bar.each_call(matching!()).returns(2),
//! ));
//!
//! assert_eq!(42, my_func(&mocked_deps));
//! ```
//!
//! #### Deep integration testing with unimock
//! Entrait with unimock supports _un-mocking_. This means that the test environment can be _partially mocked!_
//!
//! ```rust
//! # use entrait::entrait_export as entrait;
//! # use unimock::*;
//! #[entrait(SayHello)]
//! fn say_hello(deps: &impl FetchPlanetName, planet_id: u32) -> Result<String, ()> {
//!     Ok(format!("Hello {}!", deps.fetch_planet_name(planet_id)?))
//! }
//!
//! #[entrait(FetchPlanetName)]
//! fn fetch_planet_name(deps: &impl FetchPlanet, planet_id: u32) -> Result<String, ()> {
//!     let planet = deps.fetch_planet(planet_id)?;
//!     Ok(planet.name)
//! }
//!
//! pub struct Planet {
//!     name: String
//! }
//!
//! #[entrait(FetchPlanet, mock_api=FetchPlanetMock)]
//! fn fetch_planet(deps: &(), planet_id: u32) -> Result<Planet, ()> {
//!     unimplemented!("This doc test has no access to a database :(")
//! }
//!
//! let hello_string = say_hello(
//!     &Unimock::new_partial(
//!         FetchPlanetMock
//!             .some_call(matching!(123456))
//!             .returns(Ok(Planet {
//!                 name: "World".to_string(),
//!             }))
//!     ),
//!     123456,
//! ).unwrap();
//!
//! assert_eq!("Hello World!", hello_string);
//! ```
//!
//! This example used [`Unimock::new_partial`](unimock::Unimock::new_partial) to create a mocker that works mostly like `Impl`, except that the call graph can be short-circuited at arbitrary, run-time configurable points.
//! The example code goes through three layers (`say_hello => fetch_planet_name => fetch_planet`), and only the deepest one gets mocked out.
//!
//!
//! ### Alternative mocking: Mockall
//! If you instead wish to use a more established mocking crate, there is also support for [mockall](https://docs.rs/mockall/latest/mockall/).
//! Note that mockall has some limitations.
//! Multiple trait bounds are not supported, and deep tests will not work.
//! Also, mockall tends to generate a lot of code, often an order of magnitude more than unimock.
//!
//! Enabling mockall is done using the `mockall` entrait option.
//! There is no cargo feature to turn this on implicitly, because mockall doesn't work well when it's re-exported through another crate.
//!
//! ```rust
//! # use entrait::entrait_export as entrait;
//! #[entrait(Foo, mockall)]
//! fn foo<D>(_: &D) -> u32 {
//!     unimplemented!()
//! }
//!
//! fn my_func(deps: &impl Foo) -> u32 {
//!     deps.foo()
//! }
//!
//! fn main() {
//!     let mut deps = MockFoo::new();
//!     deps.expect_foo().returning(|| 42);
//!     assert_eq!(42, my_func(&deps));
//! }
//! ```
//!
//!
//! # Multi-crate architecture
//!
//! A common technique for Rust application development is to choose a multi-crate architecture.
//! There are usually two main ways to go about it:
//!
//! 1. The call graph and crate dependency go in the same direction.
//! 2. The call graph and crate dependency go in _opposite_ directions.
//!
//! The first option is how libraries are normally used: Its functions are just called, without any indirection.
//!
//! The second option can be referred to as a variant of the
//!     [dependency inversion principle](https://en.wikipedia.org/wiki/Dependency_inversion_principle).
//! This is usually a desirable architectural property, and achieving this with entrait is what this section is about.
//!
//! The main goal is to be able to express business logic _centrally_, and avoid depending directly on infrastructure details (onion architecture).
//! All of the examples in this section make some use of traits and trait delegation.
//!
//!
//! ### Case 1: Concrete leaf dependencies
//! Earlier it was mentioned that when concrete-type dependencies are used, the `T` in `Impl<T>`, your application, and the type of the dependency have to match.
//! But this is only partially true.
//! It really comes down to which traits are implemented on what types:
//!
//! ```rust
//! # use entrait::*;
//! pub struct Config {
//!     foo: String,
//! }
//!
//! #[entrait_export(pub GetFoo)]
//! fn get_foo(config: &Config) -> &str {
//!     &config.foo
//! }
//! ```
//!
//! <details>
//! <summary>🔬 <strong>Inspect the generated code</strong> 🔬</summary>
//!
//! ```rust
//! # use ::entrait::Impl;
//! # pub struct Config { foo: String, }
//! # fn get_foo(config: &Config) -> &str { &config.foo }
//! trait GetFoo {
//!     fn get_foo(&self) -> &str;
//! }
//! impl<T: GetFoo> GetFoo for Impl<T> {
//!     fn get_foo(&self) -> &str {
//!         self.as_ref().get_foo()
//!     }
//! }
//! impl GetFoo for Config {
//!     fn get_foo(&self) -> &str {
//!         get_foo(self)
//!     }
//! }
//! ```
//!
//! </details>
//!
//! Here we actually have a trait `GetFoo` that is implemented two times: for `Impl<T> where T: GetFoo` and for `Config`.
//! The first implementation is delegating to the other one.
//!
//! For making this work with _any_ downstream application type, we just have to manually implement `GetFoo` for that application:
//!
//! ```rust
//! # mod some_upstream_crate {
//! #     use entrait::*;
//! #     pub struct Config { pub foo: String }
//! #     #[entrait_export(pub GetFoo)]
//! #     fn get_foo(config: &Config) -> &str {
//! #         &config.foo
//! #     }
//! # }
//! struct App {
//!     config: some_upstream_crate::Config,
//! }
//! impl some_upstream_crate::GetFoo for App {
//!     fn get_foo(&self) -> &str {
//!         self.config.get_foo()
//!     }
//! }
//! ```
//!
//!
//! ### Case 2: Hand-written trait as a leaf dependency
//! Using a concrete type like `Config` from the first case can be contrived in many situations.
//! Sometimes a good old hand-written trait definition will do the job much better:
//!
//! ```rust
//! # use entrait::*;
//! #[entrait]
//! pub trait System {
//!     fn current_time(&self) -> u128;
//! }
//! ```
//!
//! <details>
//! <summary>🔬 <strong>Inspect the generated code</strong> 🔬</summary>
//!
//! ```rust
//! # use ::entrait::Impl;
//! # pub trait System: 'static { fn current_time(&self) -> u128; }
//! impl<T: System> System for Impl<T> {
//!     fn current_time(&self) -> u128 {
//!         self.as_ref().current_time()
//!     }
//! }
//! ```
//!
//! </details>
//!
//! What the attribute does in this case, is just to generate the correct blanket implementations of the trait: _delegation_ and _mocks_.
//!
//! To use with some `App`, the app type itself should implement the trait.
//!
//!
//! ### Case 3: Hand-written trait as a leaf dependency using _dynamic dispatch_
//! Sometimes it might be desirable to have a delegation that involves dynamic dispatch.
//! Entrait has a `delegate_by =` option, where you can pass an alternative trait to use as part of the delegation strategy.
//! To enable dynamic dispatch, use [`ref`](::core::convert::AsRef):
//!
//! ```rust
//! # use entrait::*;
//! #[entrait(delegate_by=ref)]
//! trait ReadConfig: 'static {
//!     fn read_config(&self) -> &str;
//! }
//! ```
//!
//! <details>
//! <summary>🔬 <strong>Inspect the generated code</strong> 🔬</summary>
//!
//! ```rust
//! # use ::entrait::Impl;
//! # trait ReadConfig: 'static { fn read_config(&self) -> &str; }
//! impl<T: ::core::convert::AsRef<dyn ReadConfig> + 'static> ReadConfig for Impl<T> {
//!     fn read_config(&self) -> &str {
//!         self.as_ref().as_ref().read_config()
//!     }
//! }
//! ```
//!
//! </details>
//!
//! To use this together with some `App`, it should implement the [`AsRef<dyn ReadConfig>`](::core::convert::AsRef) trait.
//!
//!
//! ### Case 4: Truly inverted _internal dependencies_ - static dispatch
//! All cases up to this point have been _leaf dependencies_.
//! Leaf dependencies are delegations that exit from the `Impl<T>` layer, using delegation targets involving concete `T`'s.
//! This means that it is impossible to continue to use the entrait pattern and extend your application behind those abstractions.
//!
//! To make your abstraction _extendable_ and your dependency _internal_, we have to keep the `T` generic inside the [Impl] type.
//! To make this work, we have to make use of two helper traits:
//!
//! ```rust
//! # use entrait::*;
//! #[entrait(RepositoryImpl, delegate_by = DelegateRepository)]
//! pub trait Repository {
//!     fn fetch(&self) -> i32;
//! }
//! ```
//!
//! <details>
//! <summary>🔬 <strong>Inspect the generated code</strong> 🔬</summary>
//!
//! ```rust
//! # use ::entrait::Impl;
//! # trait Repository { fn fetch(&self) -> i32; }
//! pub trait RepositoryImpl<T> {
//!     fn fetch(_impl: &Impl<T>) -> i32;
//! }
//! pub trait DelegateRepository<T> {
//!     type Target: RepositoryImpl<T>;
//! }
//! impl<T: DelegateRepository<T>> Repository for Impl<T> {
//!     fn fetch(&self) -> i32 {
//!         <T as DelegateRepository<T>>::Target::fetch(self)
//!     }
//! }
//! ```
//!
//! </details>
//!
//! This syntax introduces a total of _three_ traits:
//!
//! * `Repository`: The _dependency_, what the rest of the application directly calls.
//! * `RepositoryImpl<T>`: The _delegation target_, a trait which needs to be implemented by some `Target` type.
//! * `DelegateRepository<T>`: The _delegation selector_, that selects the specific `Target` type to be used for some specific `App`.
//!
//! This design makes it possible to separate concerns into three different crates, ordered from most-upstream to most-downstream:
//! 1. _Core logic:_ Depend on and call `Repository` methods.
//! 2. _External system integration:_ Provide some implementation of the repository, by implementing `RepositoryImpl<T>`.
//! 3. _Executable:_ Construct an `App` that selects a specific repository implementation from crate 2.
//!
//! All delegation from `Repository` to `RepositoryImpl<T>` goes via the `DelegateRepository<T>` trait.
//! The method signatures in `RepositoryImpl<T>` are _static_, and receives the `&Impl<T>` via a normal parameter.
//! This allows us to continue using entrait patterns within those implementations!
//!
//! In _crate 2_, we have to provide an implementation of `RepositoryImpl<T>`.
//! This can either be done manually, or by using the [entrait] attribute on an `impl` block:
//!
//! ```rust
//! # mod demo {
//! # use entrait::*;
//! # mod crate1 {
//! #     use super::*;
//! #     pub trait RepositoryImpl<T> { fn fetch(_impl: &Impl<T>) -> i32; }
//! # }
//! pub struct MyRepository;
//!
//! #[entrait]
//! impl crate1::RepositoryImpl for MyRepository {
//!     // this function has the now-familiar entrait-compatible signature:
//!     fn fetch<D>(deps: &D) -> i32 {
//!         unimplemented!()
//!     }
//! }
//! # } // demo
//! ```
//!
//! <details>
//! <summary>🔬 <strong>Inspect the generated code</strong> 🔬</summary>
//!
//! ```rust
//! # mod demo {
//! # use entrait::*;
//! # mod crate1 {
//! #     use super::*;
//! #     pub trait RepositoryImpl<T> { fn fetch(_impl: &Impl<T>) -> i32; }
//! # }
//! # pub struct MyRepository;
//! impl MyRepository {
//!     fn fetch<D>(deps: &D) -> i32 {
//!         unimplemented!()
//!     }
//! }
//! impl<T> crate1::RepositoryImpl<T> for MyRepository {
//!     #[inline]
//!     fn fetch(_impl: &Impl<T>) -> i32 {
//!         Self::fetch(_impl)
//!     }
//! }
//! # } // demo
//! ```
//!
//! </details>
//!
//! Entrait will split this trait implementation block in two: An _inherent_ one containing the original code, and a proper trait implementation which performs the delegation.
//!
//! In the end, we just have to implement our `DelegateRepository<T>`:
//!
//! ```rust
//! # mod demo {
//! # use entrait::*;
//! # mod crate1 { pub trait DelegateRepository<T> { type Target; } }
//! # mod crate2 { pub struct MyRepository; }
//! // in crate3:
//! struct App;
//! impl crate1::DelegateRepository<Self> for App {
//!     type Target = crate2::MyRepository;
//! }
//! fn main() { /* ... */ }
//! # } // demo
//! ```
//!
//!
//! ### Case 5: Truly inverted internal dependencies - dynamic dispatch
//! A small variation of case 4: Use `delegate_by=ref` instead of a custom trait.
//! This makes the delegation happen using dynamic dispatch.
//!
//! The implementation syntax is almost the same as in case 4, only that the entrait attribute must now be `#[entrait(ref)]`:
//!
//! ```rust
//! # mod demo {
//! # use entrait::*;
//! #[entrait(RepositoryImpl, delegate_by=ref)]
//! pub trait Repository {
//!     fn fetch(&self) -> i32;
//! }
//!
//! pub struct MyRepository;
//!
//! #[entrait(ref)]
//! impl RepositoryImpl for MyRepository {
//!     fn fetch<D>(deps: &D) -> i32 {
//!         unimplemented!()
//!     }
//! }
//! # } // demo
//! ```
//!
//! The app must now implement [`AsRef<dyn RepositoryImpl<Self>>`](::core::convert::AsRef).
//!
//!
//!
//!
//! # Options and features
//!
//! #### Trait visibility
//! by default, entrait generates a trait that is module-private (no visibility keyword).
//! To change this, just put a visibility specifier before the trait name:
//!
//! ```rust
//! use entrait::*;
//! #[entrait(pub Foo)]   // <-- public trait
//! fn foo<D>(deps: &D) { // <-- private function
//! }
//! ```
//!
//! #### async support
//! Zero-cost, static-dispatch `async` works out of the box[^1].
//!
//! When dynamic dispatch is needed, for example in combination with `delegate_by=ref`, entrait understands the `#[async_trait]` attribute when applied _after_ the entrait macro.
//! Entrait will re-apply that macro to the various generated impl blocks as needed.
//!
//! ##### async `Send`-ness
//! Similar to `async_trait`, entrait generates a [Send]-bound on futures by default.
//! To opt out of the Send bound, pass `?Send` as a macro argument:
//!
//! ```rust
//! # use std::{rc::Rc, any::Any};
//! # use entrait::*;
//! #[entrait(ReturnRc, ?Send)]
//! async fn return_rc(_deps: impl Any) -> Rc<i32> {
//!     Rc::new(42)
//! }
//! ```
//!
//! #### Integrating with other `fn`-targeting macros, and `no_deps`
//! Some macros are used to transform the body of a function, or generate a body from scratch.
//! For example, we can use [`feignhttp`](https://docs.rs/feignhttp/latest/feignhttp/) to generate an HTTP client. Entrait will try as best as it
//! can to co-exist with macros like these. Since `entrait` is a higher-level macro that does not touch fn bodies (it does not even try to parse them),
//! entrait should be processed after, which means it should be placed _before_ lower level macros. Example:
//!
//! ```rust
//! # use entrait::entrait;
//! #[entrait(FetchThing, no_deps)]
//! #[feignhttp::get("https://my.api.org/api/{param}")]
//! async fn fetch_thing(#[path] param: String) -> feignhttp::Result<String> {}
//! ```
//!
//! Here we had to use the `no_deps` entrait option.
//! This is used to tell entrait that the function does not have a `deps` parameter as its first input.
//! Instead, all the function's inputs get promoted to the generated trait method.
//!
//! #### Conditional compilation of mocks
//! Most often, you will only need to generate mock implementations for test code, and skip this for production code.
//! A notable exception to this is when building libraries.
//! When an application consists of several crates, downstream crates would likely want to mock out functionality from libraries.
//!
//! Entrait calls this _exporting_, and it unconditionally turns on autogeneration of mock implementations:
//!
//! ```
//! # use entrait::*;
//! #[entrait_export(pub Bar)]
//! fn bar(deps: &()) {}
//! ```
//! or
//! ```
//! # use entrait::*;
//! #[entrait(pub Foo, export)]
//! fn foo(deps: &()) {}
//! ```
//!
//! It is also possible to reduce noise by doing `use entrait::entrait_export as entrait`.
//!
//! #### Feature overview
//! | Feature                  | Implies         | Description         |
//! | -------------------      | --------------- | ------------------- |
//! | `unimock`                |                 | Adds the [unimock] dependency, and turns on Unimock implementations for all traits. |
//!
//!
//! # "Philosophy"
//! The `entrait` crate is central to the _entrait pattern_, an opinionated yet flexible and _Rusty_ way to build testable applications/business logic.
//!
//! To understand the entrait model and how to achieve Dependency Injection (DI) with it, we can compare it with a more widely used and classical alternative pattern:
//!     _Object-Oriented DI_.
//!
//! In object-oriented DI, each named dependency is a separate object instance.
//! Each dependency exports a set of public methods, and internally points to a set of private dependencies.
//! A working application is built by fully instantiating such an _object graph_ of interconnected dependencies.
//!
//! Entrait was built to address two drawbacks inherent to this design:
//!
//! * Representing a _graph_ of objects (even if acyclic) in Rust usually requires reference counting/heap allocation.
//! * Each "dependency" abstraction often contains a lot of different functionality.
//!     As an example, consider [DDD](https://en.wikipedia.org/wiki/Domain-driven_design)-based applications consisting of `DomainServices`.
//!     There will typically be one such class per domain object, with a lot of methods in each.
//!     This results in dependency graphs with fewer nodes overall, but the number of possible _call graphs_ is much larger.
//!     A common problem with this is that the _actual dependencies_—the functions actually getting called—are encapsulated
//!         and hidden away from public interfaces.
//!     To construct valid dependency mocks in unit tests, a developer will have to read through full function bodies instead of looking at signatures.
//!
//! `entrait` solves this by:
//!
//! * Representing dependencies as _traits_ instead of types, automatically profiting from Rust's builtin zero-cost abstraction tool.
//! * Giving users a choice between fine and coarse dependency granularity, by enabling both single-function traits and module-based traits.
//! * Always declaring dependencies at the function signature level, close to call sites, instead of at module level.
//!
//!
//! # Limitations
//! This section lists known limitations of entrait:
//!
//! ### Cyclic dependency graphs
//! Cyclic dependency graphs are impossible with entrait.
//! In fact, this is not a limit of entrait itself, but with Rust's trait solver.
//! It is not able to prove that a type implements a trait if it needs to prove that it does in order to prove it.
//!
//! While this is a limitation, it is not necessarily a bad one.
//! One might say that a layered application architecture should never contain cycles.
//! If you do need recursive algorithms, you could model this as utility functions outside of the entraited APIs of the application.
//!
//! [^1]: Literally, out of the [Box]! In entrait version 0.7 and newer, asynchronous functions are zero-cost by default.

#![forbid(unsafe_code)]

#[cfg(feature = "unimock")]
mod macros {
    pub use entrait_macros::entrait_export_unimock as entrait_export;
    pub use entrait_macros::entrait_unimock as entrait;
}

#[cfg(not(feature = "unimock"))]
mod macros {
    pub use entrait_macros::entrait;
    pub use entrait_macros::entrait_export;
}

/// The entrait attribute macro, used to generate traits and _delegating implementations_ of them.
///
/// ## For functions
/// When used with a function, the macro must be given the name of a trait to generate.
/// The macro will generate that trait, and connect it to the function by supplying an implementation for [Impl], plus optional mock implementations.
///
/// #### Syntax
///
/// ```no_compile
/// #[entrait($visibility? $TraitIdent)]
/// fn ...
/// ```
///
/// * `$visibility`: Optional visibility specifier for the generated trait.
///     See the [Rust documentation](https://doc.rust-lang.org/reference/visibility-and-privacy.html) for valid values.
/// * `$TraitIdent`: Any valid Rust identifier that starts with an upper-case character, used as the name of the new trait.
///
/// with options:
///
/// ```no_compile
/// #[entrait($visibility? $TraitIdent, $option, ...)]
/// fn ...
/// ```
///
/// ## For modules
/// Using the attribute on a module is used to group several non-private functions into one trait.
/// Only non-private functions are considered by the macro.
///
/// #### Syntax
/// ```no_compile
/// #[entrait($visibility? $TraitIdent)]
/// mod some_module {
///     pub fn ...
/// }
/// ```
///
///
/// ## For traits
/// When used with a trait, the macro will only provide a delegating implementation for [Impl] that delegates to another trait implementation.
/// It can also optionally generate mock impls of the trait.
///
/// There are mainly two delegation modes:
///
/// 1. Specify a trait name to use as delegation target, resulting in an _internal dependency_.
/// 2. Don't specify a trait name, resulting in a _leaf dependency_ which has to delegate using the same trait, but for a different type.
///
/// When mocking is enabled, exporting the mocks is implicitly turned on (i.e. not gated by `#[cfg(test)]`).
///
/// #### Syntax
///
/// ```no_compile
/// #[entrait($visibility? $TraitIdent?)]
/// trait ...
/// ```
///
/// with options:
///
/// ```no_compile
/// #[entrait($visibility? $TraitIdent?, $option, ...)]
/// trait ...
/// ```
///
/// ##### Example 1
/// Internal dependency, static dispatch (delegation bound: `T: DelegateFoo<T>`):
/// ```rust
/// # use entrait::*;
/// #[entrait(FooImpl, delegate_by = DelegateFoo)]
/// trait Foo {}
/// ```
/// Note: The associated type `DelegateFoo<T>::Target` must implement `FooImpl<T>`.
///
/// ##### Example 2
/// Leaf dependency, static dispatch (delegation bound: `T: Foo`):
/// ```rust
/// # use entrait::*;
/// #[entrait]
/// trait Foo {}
/// ```
///
/// ##### Example 3
/// Leaf dependency, dynamic dispatch (delegation bound: `T: AsRef<dyn Foo>`):
/// ```rust
/// # use entrait::*;
/// #[entrait(delegate_by=ref)]
/// trait Foo {}
/// ```
///
///
/// ## For impl blocks
/// When used on an impl block, the macro will generate a delegating implementation for a _delegation trait_ "`TraitImpl`" generated with `#[entrait(TraitImpl)] trait Trait {}`.
///
/// The impl block must be be a trait implementation, not an inherent implementation.
/// Within this block, only static methods with a dependency receiver are supported.
/// These methods must correspond with the method definitions from the trait that is implemented.
/// I.e. the method signatures have to match, except for `self` receivers which must be replaced by dependency receivers.
///
/// The macro will convert the impl block from a trait implementation to an inherent implementation block, and additionally generate a proper trait implementation block based on the methods present.
///
/// Given the trait declaration:
///
/// ```rust
/// # use entrait::*;
/// #[entrait(TraitImpl, delegate_by = DelegateTrait)]
/// trait Trait {
///     fn foo(&self, arg: i32) -> i32;
/// }
/// ```
///
/// The delegation chain is fulfilled with a block like the following:
///
/// ```rust
/// # use entrait::*;
/// # #[entrait(TraitImpl, delegate_by = DelegateTrait)]
/// # trait Trait {
/// #     fn foo(&self, arg: i32) -> i32;
/// # }
/// pub struct MyType;
///
/// #[entrait]
/// impl TraitImpl for MyType {
///     fn foo(_deps: &impl std::any::Any, arg: i32) -> i32 {
///         arg * 2
///     }
/// }
///
/// struct App;
///
/// // The type `MyType` may now be used as a delegation target:
/// impl DelegateTrait<Self> for App {
///     type Target = MyType;
/// }
///
/// assert_eq!(42, Impl::new(App).foo(21));
/// ```
///
/// ##### `dyn trait` delegation with `AsRef`:
/// The only attribute parameter currently supported on impl blocks is adding the `ref` keyword, to indicate that the delegation strategy uses dynamic dispatch through `AsRef`:
///
/// ```rust
/// # use entrait::*;
/// #[entrait(TraitImpl, delegate_by=ref)]
/// trait Trait {
///     fn foo(&self, arg: i32) -> i32;
/// }
///
/// struct MyType;
///
/// #[entrait(ref)]
/// impl TraitImpl for MyType {
///     fn foo(_deps: &impl std::any::Any, arg: i32) -> i32 {
///         arg * 2
///     }
/// }
/// ```
///
/// #### Syntax
/// ```no_compile
/// #[entrait(ref?)]
/// impl TraitPath for Type {
///     ...
/// }
/// ```
///
///
///
/// # Options
/// An option can be just `$option` or `$option = $value`. An option without value means `true`.
///
/// | Option              | Type                      | Target             | Default     | Description         |
/// | ------------------- | ------------------------- | ------------------ | ----------- | ------------------- |
/// | `no_deps`           | `bool`                    | `fn`               | `false`     | Disables the dependency parameter, so that the first parameter is just interpreted as a normal function parameter. Useful for reducing noise in some situations. |
/// | `export`            | `bool`                    | `fn`+`mod`         | `false`     | If mocks are generated, exports these mocks even in release builds. Only relevant for libraries. |
/// | `mock_api`          | `ident`                   | `fn`+`mod`+`trait` |             | The identifier to use for mock APIs (for libraries that support custom identifiers. The `unimock` library requires this to be explicitly specified. |
/// | `unimock`           | `bool`                    | `fn`+`mod`+`trait` | `false`[^1] | Used to turn _off_ unimock implementation when the `unimock` _feature_ is enabled. |
/// | `mockall`           | `bool`                    | `fn`+`mod`+`trait` | `false`     | Enable mockall mocks. |
/// | `delegate_by`       | `Self`/`ref`/custom ident | `trait`            | `Self`      | Controls the generated `Impl<T>` delegation of this trait. `Self` generates a `T: Trait` bound. `ref` generates a [`T: AsRef<dyn Trait>`](::core::convert::AsRef) bound. `Borrow` is deprecated and uses the [core::borrow::Borrow] trait. Any other value generates a new trait with that name which controls the delegation. |
/// | `?Send`             | `true`                    | `fn`+`mod`+`trait` | `false`     | Opts out of `Send` bounds for Future outputs from `async` functions in generated traits.|
///
/// [^1]: Enabled by default by turning on the `unimock` cargo feature.
pub use macros::entrait;

/// Same as the [`entrait`](entrait) macro, only that the `export` option is set to true.
///
/// This can be used in libraries to export mocks.
///
/// A good way to reduce noise can to to import it as `use entrait::entrait_export as entrait;`.
pub use macros::entrait_export;

/// Re-exported from the [implementation] crate.
pub use ::implementation::Impl;

/// Optional mock re-exports for macros
#[cfg(feature = "unimock")]
#[doc(hidden)]
pub use ::unimock as __unimock;
