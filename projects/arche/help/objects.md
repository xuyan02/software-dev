对象共同规则：

  qname    全局唯一，格式 <parent>/<name>
  name     本地 key，不能含空格、/、.
  parent   父对象 qname；Root 没有 parent

方法 qname：

  <parent>/<name>#<hash8>

hash8 由参数类型和修饰符计算；参数只取类型，不含参数名。

技术对象：

  technical                        TechnicalRoot
  technical/<system>               TechnicalSystem
  technical/<system>/<module>      TechnicalModule
  technical/<system>/<layer>       TechnicalLayer
  technical/directory/<path>       TechnicalDirectory
  technical/file/<path>            TechnicalFile
  technical/namespace/<name>       TechnicalNamespace
  <namespace>/<name>               TechnicalClass / TechnicalStruct / TechnicalEnum /
                                   TechnicalInterface / TechnicalMixin / TechnicalFunction
  <owner>/<name>                   TechnicalVariable / TechnicalConstant / TechnicalMember /
                                   TechnicalProperty / TechnicalEnumValue /
                                   TechnicalArgument / TechnicalReturnValue
  <type>/<name>#<hash8>            TechnicalMethod
  <class>/<random>                 TechnicalPart

项目对象：

  project                                 ProjectRoot
  project/document                        ProjectDocument
    document                              DocumentParagraphList

需求对象：

  requirement
    requirement/user/<role-id>             RequirementUser（假想角色）
    requirement/story/<name>               RequirementStory
    requirement/system/<name>              RequirementSystem
    <system>/<name>                        RequirementModule
    <module>/<name>                        RequirementFunction
    <function>/<name>                      RequirementInteraction

概念对象：

  concept
    concept/domain
    concept/requirement
    concept/technical
    concept/<dir>/<key>                    Concept
    concept/<dir>/<subdir>                 ConceptDirectory

共享/文档对象：

  root
  <owner>/<field>                       DocumentText / DocumentList / DocumentParagraphList
  <list>/<random>                       DocumentTopic
  <owner>/<random>                      DocumentCode / DocumentMermaid / DocumentHtml
