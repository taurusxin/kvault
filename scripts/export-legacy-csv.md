# 临时导出旧版保险库

先保存所有记录并退出 KVault，在项目根目录运行：

```sh
uv run scripts/export_legacy_csv.py
```

按提示输入主密码（终端不会回显）。脚本自动定位本机原始 v1 保险库，只读解密，不启动 App、不连接云端、不修改或清空源文件。依赖由 uv 按脚本声明安装，不需要激活 Python 环境。

默认输出到项目下已被 Git 忽略的 `.private-exports/kvault-时间-随机值/`：

- `records.csv`：`table,key,value,notes`，包含所有未删除分类中的未删除记录。
- `tables.csv`：`id,name`，包含空表在内的未删除分类清单。
- `conflicts.csv`：仅存在未解决冲突时生成，包含本地与云端双方内容及删除标记；`side` 区分版本，`kind=absent` 表示该侧无实体。

CSV 使用带 BOM 的 UTF-8，保留中文、逗号、引号及多行内容。为保留原值，不会修改以 `=`、`+` 等开头的文本；用表格软件查看时应按纯文本导入，避免被解释为公式。表分类保存在 `table` 列；当前 App 的导入入口以选中表为目标，不能自动按此列恢复全部分类。

指定其他保险库或旧版加密快照备份：

```sh
uv run scripts/export_legacy_csv.py --vault /absolute/path/vault.kvault --output-dir /absolute/path/new-export
```

指定的输出目录必须不存在、父目录必须存在。POSIX 系统新建目录权限为 `0700`、CSV 为 `0600`；Windows 权限继承目标目录的 ACL。密码错误或认证失败不会创建 CSV，不会覆盖已有输出。

**导出内容为明文敏感数据，请妥善保管，完成恢复后删除。** 云同步凭据、旧的同步基线和已删除记录不在常规导出范围内。CSV 仅用于保留业务记录，不替代原始加密保险库备份。脚本本身不会清理任何数据；确认导出完整后再进行新架构调试前的数据重置。
