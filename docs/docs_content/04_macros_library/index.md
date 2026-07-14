
**GritShield** brings Spring Boot/JPA-style **declarative development** to Rust, but with a critical twist: **all the magic happens at compile time**.

Spring boot parses method names at runtime, Rust's type system make that approach awkward and less idiomatic, so we can achieve the same developer experience with compile-time code generation.

**Exactly the same Spring Boot patterns** — but at compile time:

|Spring Boot Pattern|GritShield Equivalent|Runtime Cost|
|---|---|---|
|`@Entity` + Repository|`#[derive(GritModel)]`|**Zero** (compile-time code gen)|
|`@Autowired`|`#[derive(GritComponent)]`|**Zero** (compile-time DI)|
|`@RestController` + `@GetMapping`|`#[controller]` + `#[get]`|**Zero** (compile-time routes)|
|`@OneToMany` / `@ManyToOne`|`#[derive(GritRelation)]`|**Zero** (compile-time builder)|
|`@Schema` (OpenAPI)|`#[derive(GritSchema)]`|**Zero** (compile-time docs)|
|Admin panels|`#[derive(GritAdmin)]`|**Zero** (compile-time UI)|
|Custom admin actions|`#[action]`|**Zero** (compile-time registration)|
