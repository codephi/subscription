# Subscription Admin UI

Painel administrativo interno do serviço Subscription. O inventário de telas,
fluxos, integração com a API, arquitetura e limites está na
[documentação funcional](../docs/frontend-administrativo.md). O plano por fases
está em [plano do frontend](../docs/plano-frontend-administrativo.md).

O painel inclui visão geral de Billing, consulta e criação de accounts, planos e
carteiras, investigação de filas de cobrança, catálogo, ações administrativas,
auditoria e inbox de eventos Accounts. Ele não tem autenticação e deve ficar
restrito à rede local ou VPN.

## Executar

Siga as [instruções do repositório](../README.md#painel-administrativo) para
subir também a API. Com a API acessível em `127.0.0.1:3000`:

```sh
npm ci
npm run dev
```

O Vite encaminha `/v1` e `/health` para a API local. Configure
`VITE_API_BASE_URL` para usar outra origem; não coloque segredos em variáveis
`VITE_`, porque elas são incluídas no frontend.

## Scripts

```sh
npm run api:types # Atualiza snapshot OpenAPI e tipos TypeScript
npm test
npm run test:e2e
npm run lint
npm run build
```

Os testes Playwright usam respostas HTTP simuladas; veja a documentação funcional
para conhecer a cobertura e os limites dos testes.
