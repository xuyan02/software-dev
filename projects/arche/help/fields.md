字段类型：

  SymbolHash        名称/qname 引用；访问器返回 StringRef
  ObjectHash        对象引用；访问器返回 ObjectRef
  Vec<SymbolHash>   名称/qname 列表；StringRefList
  Vec<ObjectHash>   对象列表；ObjectRefList
  String            普通文本
  bool              布尔标记
  u8                小整数
  SourcePath        文件路径
  SourceSpan        文件路径 + start + end
  GitCommit         源码 Git commit

文档字段目标类型：

  position, prototype, returns, form          DocumentText
  responsibilities, features, needs, ...      DocumentList
  usage, design_points, scenarios, ...        DocumentParagraphList
  flow                                        DocumentMermaid

DocumentParagraphList 的元素可以是：

  DocumentText
  DocumentList
  DocumentTopic
  DocumentCode
  DocumentMermaid
  DocumentHtml

常用技术对象字段：

ProjectRoot
  document      ProjectDocument

ProjectDocument
  document      DocumentParagraphList（定位、背景、目标、非目标、分期计划、约束、风险）

TechnicalRoot
  file          ObjectList<TechnicalFile>
  directory     ObjectList<TechnicalDirectory>
  namespace     ObjectList<TechnicalNamespace>
  system        ObjectList<TechnicalSystem>

TechnicalSystem
  members, modules, layers, constants, variables
  position, responsibilities, non_responsibilities, features, non_features,
  invariants, constraints, design_principles, external_dependencies,
  usage, design_points

TechnicalModule
  members, dependencies, constants, variables
  position, responsibilities, non_responsibilities, features, non_features,
  invariants, constraints, design_principles, usage, design_points

TechnicalLayer
  members, dependencies, position

TechnicalDirectory
  source: SourcePath, children, position

TechnicalFile
  source: SourcePath, entities, position

TechnicalNamespace
  children, entities, constants, variables, position

TechnicalClass
  namespace, file, module, source, superclass, interfaces, mixins,
  explicit_dependencies, implicit_dependencies, method, types, parts,
  constants, members, properties, position, responsibilities,
  non_responsibilities, features, non_features, invariants, constraints,
  design_principles, usage, design_points

TechnicalStruct
  namespace, file, module, source, dependencies, method, types, constants,
  members, position, invariants

TechnicalEnum
  namespace, file, module, source, dependencies, values, position

TechnicalInterface
  namespace, file, module, source, extends, dependencies, methods, position,
  responsibilities, non_responsibilities, features, non_features, invariants,
  usage, design_points

TechnicalMethod
  source, is_static, is_async, is_abstract, is_const, overrides,
  explicit_dependencies, implicit_dependencies, position, arguments,
  return_value, usage, design_points

TechnicalFunction
  namespace, file, module, source, is_async, explicit_dependencies,
  implicit_dependencies, position, arguments, return_value, usage, design_points

TechnicalMixin
  namespace, file, module, source, on, superclass, interfaces, mixins,
  explicit_dependencies, implicit_dependencies, methods, types, position,
  responsibilities, non_responsibilities, features, non_features, invariants,
  usage, design_points

TechnicalPart
  source, explicit_dependencies, implicit_dependencies, members, methods, types

TechnicalVariable
  source, ty, position

TechnicalConstant
  source, ty, position

TechnicalMember
  source, ty, is_static, is_const, position

TechnicalProperty
  ty, getter, setter, position

TechnicalEnumValue
  source, position

TechnicalArgument
  source, ty, position

TechnicalReturnValue
  source, ty, position
