@echo off
set "resonate_java_command=java"
if defined JAVA_HOME set "resonate_java_command=%JAVA_HOME%\bin\java.exe"
"%resonate_java_command%" -classpath "%~dp0gradle\wrapper\gradle-wrapper.jar" org.gradle.wrapper.GradleWrapperMain %*
